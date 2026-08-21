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

/// Which stage of the pipeline (spec §6) found it. An asset problem without
/// its stage is a bug report without a line number: "barrel.glb: failed" reads
/// very differently depending on whether the identity sidecar is corrupt, a
/// validator objected, or a processor fell over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Import,
    Validate,
    Process,
    Cook,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Self::Import => "import",
            Self::Validate => "validate",
            Self::Process => "process",
            Self::Cook => "cook",
        }
    }
}

/// Who said so. Most ingest problems come from no registered extension at all —
/// an unreadable directory, a corrupt sidecar — so a mandatory validator id
/// would be a lie for five of the six producers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ProblemSource {
    Validator(ValidatorId),
    Processor(String),
    /// The walk itself: unreadable directories, unclaimed extensions, an
    /// identity that could not be written.
    Ingest,
}

impl ProblemSource {
    /// What the problems panel shows in its second column, the way a level
    /// problem shows its validator id there.
    pub fn label(&self, stage: Stage) -> String {
        match self {
            Self::Validator(id) => id.as_str().to_string(),
            Self::Processor(id) => id.clone(),
            Self::Ingest => stage.label().to_string(),
        }
    }
}

/// A problem with an ASSET, as opposed to `LevelProblem`'s problem with an
/// entity in the level.
///
/// Deliberately FLAT rather than a wrapper around `Problem`: a validator knows
/// its own id and severity and nothing else — not which stage is running, not
/// which uuid the file was assigned, not whether the file even got one. The
/// ingest walk knows those, so it is the walk that builds the record.
///
/// `path` is asset-server-relative and forward-slashed, the same spelling
/// `ModelEntry::asset_path` carries, so a problem joins to a library entry
/// without inventing a second path convention.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct AssetProblem {
    pub stage: Stage,
    pub source: ProblemSource,
    pub severity: Severity,
    pub path: String,
    /// `None` when the failure happened BEFORE an identity existed — an import
    /// that could not write its sidecar has no uuid to point at, and inventing
    /// one would orphan every reference to the real asset.
    pub uuid: Option<uuid::Uuid>,
    pub message: String,
}

impl AssetProblem {
    /// Lift a validator's finding into an ingest record, keeping its severity
    /// exactly. The old code stringified this as `"{severity:?}: {message}"`,
    /// which is why an ignored `.fbx` and a corrupt mesh looked identical.
    pub fn from_validator(problem: Problem, path: String, uuid: Option<uuid::Uuid>) -> Self {
        Self {
            stage: Stage::Validate,
            source: ProblemSource::Validator(problem.validator),
            severity: problem.severity,
            path,
            uuid,
            message: problem.message,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A validator knows its id and its severity and nothing else — not the
    /// stage, not the uuid, not even the asset-relative path. The lift is
    /// where those arrive, and it must not quietly reinterpret the severity
    /// on the way, which is exactly what stringifying it used to do.
    #[test]
    fn lifting_a_validator_problem_keeps_what_the_validator_said() {
        let problem = Problem {
            validator: ValidatorId::new_static("gltf.units"),
            severity: Severity::Warning,
            message: "scale looks like centimetres".into(),
        };
        let id = uuid::Uuid::new_v4();
        let lifted = AssetProblem::from_validator(problem, "models/barrel.glb".into(), Some(id));
        assert_eq!(lifted.severity, Severity::Warning);
        assert_eq!(lifted.stage, Stage::Validate);
        assert_eq!(lifted.uuid, Some(id));
        assert_eq!(lifted.path, "models/barrel.glb");
        assert!(matches!(
            &lifted.source,
            ProblemSource::Validator(v) if v.as_str() == "gltf.units"
        ));
        assert_eq!(lifted.message, "scale looks like centimetres");
    }

    /// The panel's second column: a level problem shows its validator id
    /// there, so an asset problem shows whoever spoke — and for the walk
    /// itself, which has no id, the stage is the honest answer.
    #[test]
    fn every_source_can_name_itself() {
        assert_eq!(
            ProblemSource::Validator(ValidatorId::new_static("asset.nonempty"))
                .label(Stage::Validate),
            "asset.nonempty"
        );
        assert_eq!(
            ProblemSource::Processor("gltf.bounds".into()).label(Stage::Process),
            "gltf.bounds"
        );
        assert_eq!(ProblemSource::Ingest.label(Stage::Import), "import");
    }
}
