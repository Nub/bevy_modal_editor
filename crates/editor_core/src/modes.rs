//! The modal state machine. Modes are registered data (`ModeDef`), not an enum — the
//! kernel can't know what modes feature crates bring (spec §2).

use bevy::prelude::*;
use editor_api::feature::ValidatedFeatures;
use editor_api::prelude::*;
use std::collections::HashMap;

pub const MODE_NORMAL: ModeId = ModeId::new_static("normal");

#[derive(Resource)]
pub struct Modes {
    defs: HashMap<ModeId, ModeDef>,
}

impl Modes {
    pub fn from_validated(validated: &ValidatedFeatures) -> Self {
        Self {
            defs: validated
                .modes
                .iter()
                .map(|(_, def)| (def.id.clone(), def.clone()))
                .collect(),
        }
    }
    pub fn get(&self, id: &ModeId) -> Option<&ModeDef> {
        self.defs.get(id)
    }
    pub fn iter(&self) -> impl Iterator<Item = &ModeDef> {
        self.defs.values()
    }
}

/// The active mode. Change via [`set_mode`] so `ModeChanged` always fires.
#[derive(Resource, PartialEq, Eq, Debug)]
pub struct CurrentMode(pub ModeId);

impl CurrentMode {
    /// The keymap context for the active mode (context id == mode id).
    pub fn context(&self) -> ContextId {
        ContextId::new(self.0.as_str().to_string())
    }
}

#[derive(Message, Debug)]
pub struct ModeChanged {
    pub from: ModeId,
    pub to: ModeId,
}

pub fn set_mode(to: ModeId, current: &mut CurrentMode, writer: &mut MessageWriter<ModeChanged>) {
    if current.0 != to {
        let from = current.0.clone();
        current.0 = to.clone();
        writer.write(ModeChanged { from, to });
    }
}
