//! `editor_api` — the contract through which any crate provides editor features.
//!
//! Design authority: `docs/spec/04-EDITOR-API.md`. This crate is the semver-stable
//! ecosystem surface; it depends on Bevy only and never on `editor_core`/`editor_ui`.
//!
//! Module skeleton mirrors the RFC (§2). Types land here as the M0 spikes prove them —
//! the `EditQueue` spike defines `edits`, the BSN spike informs `components`.
//!
//! Guardrail reminder (spec §8, §11): every mutation flows through `EditScope`
//! transactions; actions are data invoked as events; no side doors.

// RFC §2 layout — modules are introduced with their first real types:
// pub mod feature;     // EditorFeature, FeatureManifest, FeatureRegistry
// pub mod ids;         // FeatureId, ActionId, ContextId, ModeId, PanelId, SceneId
// pub mod actions;     // ActionDef, ActionInvoked, ParamsSpec, ActionFlags
// pub mod keymap;      // Binding, KeySequence, context layering
// pub mod edits;       // EditScope, Transaction, EditOp, EditError
// pub mod components;  // ComponentOpts, migrators, PropertyHint
// pub mod kinds;       // EntityKindDef, PreviewMode
// pub mod panels;      // PanelDecl, Placement, PanelContent
// pub mod gizmos;      // GizmoCtx, HandleId
// pub mod validate;    // ValidatorDef, Problem, Severity
// pub mod pipeline;    // ImporterDef, ProcessorDef, BakerDef, AssetKindDef
// pub mod conformance; // feature-crate CI harness
// #[cfg(feature = "ui")]
// pub mod ui;          // PanelUi, PanelCtx, WidgetKit (bevy_ui/feathers)
