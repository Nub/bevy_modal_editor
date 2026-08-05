//! Validation (RFC, M4-D2): the ONE validator registry — import-time checks,
//! extended by games and feature crates exactly like actions/panels. Failures
//! become `Problem`s surfaced to the user (problems panel / import report),
//! never silent passes.

use crate::ids::ValidatorId;
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Severity {
    /// Advisory only.
    Info,
    /// Worth fixing; import proceeds.
    Warning,
    /// The asset is not game-ready; import records it as failed validation.
    Error,
}

#[derive(Clone, Debug)]
pub struct Problem {
    pub validator: ValidatorId,
    pub severity: Severity,
    pub message: String,
}

/// What a validator sees: the source file and its bytes (parsed views are the
/// validator's own business — shared parsed caches can come later without
/// changing this contract).
pub struct ValidateCx<'a> {
    pub source: &'a Path,
    pub bytes: &'a [u8],
}

/// A problem found in the LIVE LEVEL (owner ask, v1 parity): required
/// configs/objects/components a scene must satisfy to be valid.
#[derive(Clone, Debug)]
pub struct LevelProblem {
    pub validator: ValidatorId,
    pub severity: Severity,
    pub message: String,
    /// The offending scene entity, when the problem is entity-shaped.
    pub entity: Option<crate::ids::SceneId>,
}

/// A registered level rule. The `&mut World` is for QUERY construction only —
/// level validators DIAGNOSE, they never mutate (a mutation here is a review
/// rejection; all edits flow through `EditScope`).
#[derive(Clone)]
pub struct LevelValidatorDef {
    pub id: ValidatorId,
    pub name: &'static str,
    pub validate: fn(&mut bevy::prelude::World) -> Vec<LevelProblem>,
}

#[derive(Clone)]
pub struct ValidatorDef {
    pub id: ValidatorId,
    pub name: &'static str,
    /// File extensions this validator applies to (lowercase, no dot);
    /// empty = every asset.
    pub extensions: &'static [&'static str],
    pub validate: fn(&ValidateCx) -> Vec<Problem>,
}
