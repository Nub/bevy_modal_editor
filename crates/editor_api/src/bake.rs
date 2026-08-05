//! Bake registrations (spec §6, M4-D8): bakers derive EXPENSIVE artifacts from
//! prefab source data — colliders, LODs, merged meshes, navmesh tiles. Bakes
//! are CACHES, never source of truth: delete every output and `editor bake`
//! reproduces it bit-for-bit. Artifacts are keyed by content hash of the
//! inputs × baker version; staleness is surfaced, never silently served.
//!
//! Determinism is the CONTRACT (same as processors): same template + same
//! version ⇒ byte-identical output. Seeds live in source data.

use crate::ids::BakerId;
use uuid::Uuid;

pub struct BakeCx<'a> {
    pub prefab_id: Uuid,
    pub prefab_name: &'a str,
    /// The prefab template in THE scene format (RON) — bakers parse what they
    /// need; the hash of this string is the artifact's identity input.
    pub template_ron: &'a str,
}

#[derive(Clone)]
pub struct BakerDef {
    pub id: BakerId,
    pub name: &'static str,
    /// Bumping INVALIDATES every cached artifact of this baker.
    pub version: u32,
    /// `Ok(None)` = not applicable to this prefab (no artifact written).
    pub bake: fn(&BakeCx) -> Result<Option<Vec<u8>>, String>,
}
