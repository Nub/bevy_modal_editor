//! `editor_api` — the contract through which any crate provides editor features.
//!
//! Design authority: `docs/spec/04-EDITOR-API.md` (shapes confirmed by the M0 spikes).
//! This crate is the semver-stable ecosystem surface; it depends on Bevy only and
//! never on `editor_core`/`editor_ui`.
//!
//! Guardrails (spec §8, §11): every mutation flows through `EditScope` transactions
//! (M2); actions are data invoked as `ActionInvoked` events; no side doors.

pub mod actions;
pub mod bake;
pub mod edits;
pub mod feature;
pub mod ids;
pub mod keymap;
pub mod kinds;
pub mod panels;
pub mod pipeline;
pub mod validate;

// Arriving with their milestones (RFC §2 layout):
// pub mod components;  // M2+: ComponentOpts, migrators, PropertyHint
// pub mod gizmos;      // M2: GizmoCtx, HandleId
// pub mod conformance; // grows with each subsystem
// #[cfg(feature = "ui")]
// pub mod ui;          // M1 (editor_ui shell): PanelUi, PanelCtx, WidgetKit

pub mod prelude {
    pub use crate::actions::{ActionDef, ActionFlags, ActionInvoked, InvocationSource};
    pub use crate::bake::{BakeCx, BakerDef};
    pub use crate::edits::{EditQueue, EditScope, Edited, Op, SceneIndex, Transaction};
    pub use crate::feature::{
        EditorAppExt, EditorFeature, FeatureManifest, FeatureRegistry, ModeDef, PendingFeatures,
        RegistryError, ValidatedFeatures,
    };
    pub use crate::ids::{
        ActionId, BakerId, ContextId, EntityKindId, FeatureId, GLOBAL_CONTEXT, ModeId, PanelId,
        ProcessorId, SceneId, ValidatorId,
    };
    pub use crate::keymap::{Binding, Chord, Modifiers};
    pub use crate::kinds::{EntityKindDef, InsertPreview};
    pub use crate::panels::{PanelContent, PanelDecl, Placement, PropertySource};
    pub use crate::pipeline::{ProcessCx, ProcessorDef};
    pub use crate::validate::{Problem, Severity, ValidateCx, ValidatorDef};
}
