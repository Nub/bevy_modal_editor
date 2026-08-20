//! Actions: named, declarative, invoked as events (RFC §4).
//!
//! Features declare `ActionDef`s; only the kernel's input resolver (or palette, macro
//! player, test driver) constructs `ActionInvoked`. Binding strings stay raw here and
//! are parsed/validated at registry-validation time so errors can name their action.

use crate::ids::{ActionId, ContextId};
use bevy::prelude::*;
use bevy::reflect::PartialReflect;
use std::borrow::Cow;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct ActionFlags {
    /// Handling this action MUST open exactly one `EditQueue` transaction (enforced by
    /// the conformance harness once the queue exists — M2).
    pub is_edit: bool,
    /// Hidden from the palette/cheat-sheet (internal or gesture-only actions).
    pub hidden: bool,
}

/// Which palette section this action belongs to (spec §7 palette).
///
/// The palette is the only surface in this editor that teaches the keyboard,
/// and it used to file every normal-mode action into one alphabetical bucket —
/// which put a whole workflow's verbs six letters apart and pushed the last
/// third of the list off the page entirely. A group is a domain a builder
/// reaches for, ordered by how often they reach for it.
///
/// Defaulted from the action id's namespace, so a feature that says nothing is
/// filed sensibly rather than dumped in a catch-all.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PaletteGroup(pub &'static str);

impl PaletteGroup {
    pub const PLACE: Self = Self("PLACE");
    pub const SOCKETS: Self = Self("SOCKETS & KITS");
    pub const EDIT: Self = Self("SELECT & EDIT");
    pub const PREFABS: Self = Self("PREFABS");
    pub const MATERIALS: Self = Self("MATERIALS");
    pub const ANIMATION: Self = Self("ANIMATION");
    pub const VIEW: Self = Self("VIEW & PANELS");
    pub const SCENE: Self = Self("SCENE & SESSION");

    /// Display order: what a level builder reaches for, most often first. An
    /// alphabetical palette is a filing cabinet; this is a workbench.
    pub const ORDER: [Self; 8] = [
        Self::PLACE,
        Self::SOCKETS,
        Self::EDIT,
        Self::PREFABS,
        Self::MATERIALS,
        Self::ANIMATION,
        Self::VIEW,
        Self::SCENE,
    ];

    /// Where an action lands when it never said. The id namespace already
    /// encodes the domain — `socket.generate-ends` is a socket verb whether or
    /// not anyone remembered to say so.
    pub fn from_id(id: &str) -> Self {
        let namespace = id.split('.').next().unwrap_or_default();
        match namespace {
            "insert" | "model" => Self::PLACE,
            "socket" | "chain" | "paint" => Self::SOCKETS,
            "select" | "transform" | "component" => Self::EDIT,
            "prefab" => {
                // The kit verbs are a socket workflow that happens to live on
                // prefabs; filing them apart is what scattered them.
                if matches!(
                    id,
                    "prefab.repeat" | "prefab.fill" | "prefab.paint" | "prefab.set-kit"
                ) {
                    Self::SOCKETS
                } else {
                    Self::PREFABS
                }
            }
            "material" => Self::MATERIALS,
            "anim" | "timeline" => Self::ANIMATION,
            "view" | "camera" | "panel" | "hierarchy" | "inspector" => Self::VIEW,
            _ => Self::SCENE,
        }
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}
#[derive(Clone, Debug)]
pub struct ActionDef {
    pub id: ActionId,
    pub name: Cow<'static, str>,
    pub description: Cow<'static, str>,
    /// Keymap contexts where this action is valid. Empty = global context.
    pub contexts: Vec<ContextId>,
    /// Raw binding strings ("ctrl+z", "g g") — parsed during registry validation.
    pub default_bindings: Vec<Cow<'static, str>>,
    pub flags: ActionFlags,
    /// Palette section. `None` = derive from the id namespace.
    pub group: Option<PaletteGroup>,
}

impl ActionDef {
    pub fn new(id: impl Into<ActionId>, name: &'static str) -> Self {
        Self {
            id: id.into(),
            name: Cow::Borrowed(name),
            description: Cow::Borrowed(""),
            contexts: Vec::new(),
            default_bindings: Vec::new(),
            flags: ActionFlags::default(),
            group: None,
        }
    }

    /// File this action under a palette section explicitly (spec §7).
    pub fn group(mut self, group: PaletteGroup) -> Self {
        self.group = Some(group);
        self
    }

    /// The section this action shows under, declared or derived.
    pub fn palette_group(&self) -> PaletteGroup {
        self.group
            .unwrap_or_else(|| PaletteGroup::from_id(self.id.as_str()))
    }
    pub fn describe(mut self, description: &'static str) -> Self {
        self.description = Cow::Borrowed(description);
        self
    }
    pub fn context(mut self, context: impl Into<ContextId>) -> Self {
        self.contexts.push(context.into());
        self
    }
    pub fn bind(mut self, binding: &'static str) -> Self {
        self.default_bindings.push(Cow::Borrowed(binding));
        self
    }
    pub fn edit(mut self) -> Self {
        self.flags.is_edit = true;
        self
    }
    pub fn hidden(mut self) -> Self {
        self.flags.hidden = true;
        self
    }
}

/// How an action came to be invoked — recorded for telemetry and macro semantics.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InvocationSource {
    Key,
    Palette,
    Macro,
    Script,
    Test,
}

/// The single invocation path (RFC §4: no side door). Broadcast message consumed by
/// feature systems via `MessageReader<ActionInvoked>`.
#[derive(Message)]
pub struct ActionInvoked {
    pub action: ActionId,
    /// Serializable args matching the action's params spec (macro recording rides on
    /// this being reflect-serializable). None for parameterless actions.
    pub args: Option<Box<dyn PartialReflect>>,
    pub source: InvocationSource,
}

impl ActionInvoked {
    pub fn is(&self, id: &ActionId) -> bool {
        &self.action == id
    }
}
