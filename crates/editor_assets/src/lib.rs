//! `editor_assets` — the ingestion pipeline (spec §6): Import → Validate →
//! Process → Cook.
//!
//! **Wrap-don't-fork decision (M4-D1 evaluation, ledger #10)**: Bevy 0.19's
//! `AssetProcessor` already provides hash-keyed re-processing with `.meta`
//! files — the Process stage (D3) builds ON it. What Bevy does NOT provide is
//! stable identity: assets are addressed by path, so a rename or re-import
//! breaks every reference. This crate adds the identity layer the studio
//! pipeline requires: a versioned `.import.ron` sidecar per source asset
//! carrying a UUID that SURVIVES re-import — references target the UUID, never
//! the path.

pub mod identity;

pub use identity::{
    import_file, read_identity, AssetIdentity, ImportError, IMPORT_FORMAT_VERSION,
};
