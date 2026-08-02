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
        }
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
