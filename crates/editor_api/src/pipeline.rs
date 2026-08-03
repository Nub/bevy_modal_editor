//! Pipeline registrations (RFC, M4-D3): processors — deterministic, versioned,
//! cache-keyed transformations of source assets. Registered like every other
//! extension; the Process runner (editor_assets) owns caching and invalidation.
//!
//! Determinism is the CONTRACT: same input bytes + same processor version must
//! produce byte-identical output (CI proves it on fixtures). Randomness must be
//! seeded from inputs; time/machine state are forbidden inputs.

use crate::ids::ProcessorId;
use std::path::Path;

pub struct ProcessCx<'a> {
    pub source: &'a Path,
    pub bytes: &'a [u8],
}

#[derive(Clone)]
pub struct ProcessorDef {
    pub id: ProcessorId,
    pub name: &'static str,
    /// Bumping INVALIDATES every cached output of this processor.
    pub version: u32,
    /// Extensions this processor applies to (lowercase, no dot).
    pub extensions: &'static [&'static str],
    pub process: fn(&ProcessCx) -> Result<Vec<u8>, String>,
}
