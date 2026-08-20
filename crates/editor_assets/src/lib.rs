//! `editor_assets` — the ingestion pipeline (spec §6): Import → Validate →
//! Process → Cook.
//!
//! **Wrap-don't-fork decision (M4-D1 evaluation, ledger #10)**: Bevy 0.19's
//! `AssetProcessor` already provides hash-keyed re-processing with `.meta`
//! files, and the Process stage here implements the SAME contract —
//! deterministic, content-hash keyed, version-invalidated — rather than
//! building on it, so migrating at cook time is mechanical. (The earlier
//! wording claimed this crate "builds ON" `AssetProcessor`; it does not, and
//! saying so hid how much of the pipeline was ours to keep running.)
//!
//! What Bevy does NOT provide is stable identity: assets are addressed by path,
//! so a rename or re-import breaks every reference. This crate adds the
//! identity layer the studio pipeline requires: a versioned `.import.ron`
//! sidecar per source asset carrying a UUID that SURVIVES re-import —
//! references target the UUID, never the path.

pub mod bounds;
pub mod fixture;
pub mod identity;
pub mod process;
pub mod validation;

pub use bounds::{BOUNDS_PROCESSOR, ModelBounds};
pub use editor_api::pipeline::{ProcessCx, ProcessorDef};
pub use identity::{AssetIdentity, IMPORT_FORMAT_VERSION, ImportError, import_file, read_identity};
pub use process::{ProcessError, ProcessOutcome, cache_path, process_asset};
pub use validation::{builtin_validators, run_validators};
