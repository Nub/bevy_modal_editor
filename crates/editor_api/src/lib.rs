//! `editor_api` — the contract through which any crate provides editor features.
//!
//! Design authority: `docs/spec/04-EDITOR-API.md` (shapes confirmed by the M0 spikes).
//! This crate is the semver-stable ecosystem surface; it depends on Bevy only and
//! never on `editor_core`/`editor_ui`.
//!
//! Guardrails (spec §8, §11): every mutation flows through `EditScope` transactions
//! (M2); actions are data invoked as `ActionInvoked` events; no side doors.

pub mod actions;
pub mod feature;
pub mod ids;
pub mod keymap;

// Arriving with their milestones (RFC §2 layout):
// pub mod edits;       // M2: EditScope, Transaction, EditOp
// pub mod components;  // M2: ComponentOpts, migrators, PropertyHint
// pub mod kinds;       // M2: EntityKindDef, PreviewMode
// pub mod panels;      // M1 (editor_ui shell): PanelDecl, Placement
// pub mod gizmos;      // M2: GizmoCtx, HandleId
// pub mod validate;    // M3: ValidatorDef, Problem, Severity
// pub mod pipeline;    // M4: ImporterDef, ProcessorDef, BakerDef, AssetKindDef
// pub mod conformance; // grows with each subsystem
// #[cfg(feature = "ui")]
// pub mod ui;          // M1 (editor_ui shell): PanelUi, PanelCtx, WidgetKit

pub mod prelude {
    pub use crate::actions::{ActionDef, ActionFlags, ActionInvoked, InvocationSource};
    pub use crate::feature::{
        EditorAppExt, EditorFeature, FeatureManifest, FeatureRegistry, ModeDef,
        PendingFeatures, RegistryError, ValidatedFeatures,
    };
    pub use crate::ids::{
        ActionId, ContextId, EntityKindId, FeatureId, ModeId, PanelId, SceneId,
        GLOBAL_CONTEXT,
    };
    pub use crate::keymap::{Binding, Chord, Modifiers};
}
