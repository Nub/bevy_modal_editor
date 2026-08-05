//! Keymaps as data (spec §4, M1 acceptance A5): defaults come from the validated
//! registry; a user RON file layers on top. An override *replaces* all default
//! bindings for that (context, action); `binding: None` unbinds.

use bevy::prelude::*;
use editor_api::feature::{CompiledBinding, ValidatedFeatures};
use editor_api::keymap::Binding;
use editor_api::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Where keymap layers load from. Default: no user file (pure registry defaults).
#[derive(Resource, Clone, Default)]
pub struct KeymapPaths {
    pub user: Option<PathBuf>,
}

/// The user-layer file format:
/// ```ron
/// (overrides: [
///     (context: "normal", action: "core.palette", binding: Some("p")),
///     (context: "global", action: "core.toggle-editor", binding: None), // unbind
/// ])
/// ```
#[derive(Serialize, Deserialize, Default)]
pub struct UserKeymap {
    pub overrides: Vec<UserOverride>,
}

#[derive(Serialize, Deserialize)]
pub struct UserOverride {
    pub context: String,
    pub action: String,
    pub binding: Option<String>,
}

/// The compiled dispatch table the resolver reads.
#[derive(Resource, Default)]
pub struct ResolvedKeymapData {
    pub by_context: HashMap<ContextId, Vec<(Binding, ActionId)>>,
}

#[derive(Debug)]
pub enum KeymapError {
    Io(std::io::Error),
    Parse(String),
    BadOverride {
        action: String,
        binding: String,
        message: String,
    },
}

impl std::fmt::Display for KeymapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "reading user keymap: {e}"),
            Self::Parse(e) => write!(f, "parsing user keymap: {e}"),
            Self::BadOverride {
                action,
                binding,
                message,
            } => {
                write!(
                    f,
                    "user override for {action}: binding {binding:?}: {message}"
                )
            }
        }
    }
}

/// Build the dispatch table: registry defaults, then user overrides layered on top.
pub fn build_keymap(
    validated: &ValidatedFeatures,
    paths: &KeymapPaths,
) -> Result<ResolvedKeymapData, KeymapError> {
    // Start from defaults.
    let mut entries: Vec<CompiledBinding> = validated
        .bindings
        .iter()
        .map(|b| CompiledBinding {
            context: b.context.clone(),
            binding: b.binding.clone(),
            action: b.action.clone(),
        })
        .collect();

    // Layer the user file.
    if let Some(path) = &paths.user
        && path.exists()
    {
        let text = std::fs::read_to_string(path).map_err(KeymapError::Io)?;
        let user: UserKeymap =
            ron::from_str(&text).map_err(|e| KeymapError::Parse(e.to_string()))?;
        for o in &user.overrides {
            let context = ContextId::new(o.context.clone());
            let action = ActionId::new(o.action.clone());
            // Replace: drop all defaults for this (context, action).
            entries.retain(|e| !(e.context == context && e.action == action));
            if let Some(raw) = &o.binding {
                let binding: Binding =
                    raw.parse().map_err(|e: editor_api::keymap::ParseError| {
                        KeymapError::BadOverride {
                            action: o.action.clone(),
                            binding: raw.clone(),
                            message: e.message,
                        }
                    })?;
                entries.push(CompiledBinding {
                    context,
                    binding,
                    action,
                });
            }
        }
    }

    let mut by_context: HashMap<ContextId, Vec<(Binding, ActionId)>> = HashMap::new();
    for e in entries {
        by_context
            .entry(e.context)
            .or_default()
            .push((e.binding, e.action));
    }
    Ok(ResolvedKeymapData { by_context })
}
