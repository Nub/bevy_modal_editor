//! Level validation (v1 parity, owner ask): registered rules check the LIVE
//! scene for required configs, objects, and components — a level that loads
//! is not necessarily a level that's VALID. Games extend the rule set through
//! `FeatureRegistry::level_validator`, exactly like actions and panels; the
//! editor ships builtins for the reference-breaking cases (dangling material/
//! model/prefab references).
//!
//! Results live in `LevelValidation` (the statusbar shows the counts — state,
//! not hints); `level.validate` re-runs on demand with a summary flash, and
//! edits re-validate automatically on a debounced cadence.

use bevy::prelude::*;
use editor_api::validate::{LevelProblem, LevelValidatorDef, Severity};
use editor_core::LevelValidatorCatalog;
use editor_core::prelude::*;

/// Latest validation results — derived state, rebuilt by the runner.
#[derive(Resource, Default)]
pub struct LevelValidation {
    pub problems: Vec<LevelProblem>,
    pub generation: u64,
}

impl LevelValidation {
    pub fn count(&self, severity: Severity) -> usize {
        self.problems
            .iter()
            .filter(|p| p.severity == severity)
            .count()
    }
}

#[derive(Resource, Default)]
pub(crate) struct ValidationRequests {
    /// Explicit `level.validate` — reports a summary flash.
    pub demanded: bool,
    /// Edits since the last run — revalidate silently, debounced.
    pub dirty: bool,
    pub cooldown: u32,
}

/// Re-validate at most every N frames on edit churn (v1 cadence).
const DEBOUNCE_FRAMES: u32 = 60;

pub(crate) fn collect_validation_requests(
    mut actions: MessageReader<ActionInvoked>,
    mut edited: MessageReader<Edited>,
    mut requests: ResMut<ValidationRequests>,
) {
    for invoked in actions.read() {
        if invoked.action.as_str() == "level.validate" {
            requests.demanded = true;
        }
    }
    if edited.read().next().is_some() {
        requests.dirty = true;
    }
}

pub(crate) fn run_level_validation(world: &mut World) {
    {
        let mut requests = world.resource_mut::<ValidationRequests>();
        requests.cooldown = requests.cooldown.saturating_sub(1);
        let due = requests.demanded || (requests.dirty && requests.cooldown == 0);
        if !due {
            return;
        }
        requests.dirty = false;
        requests.cooldown = DEBOUNCE_FRAMES;
    }
    let demanded = std::mem::take(&mut world.resource_mut::<ValidationRequests>().demanded);

    let validators = world.resource::<LevelValidatorCatalog>().validators.clone();
    let mut problems = Vec::new();
    for validator in &validators {
        problems.extend((validator.validate)(world));
    }
    for problem in &problems {
        match problem.severity {
            Severity::Error => warn!(
                "level validation [{}]: {}",
                problem.validator.as_str(),
                problem.message
            ),
            _ => info!(
                "level validation [{}]: {}",
                problem.validator.as_str(),
                problem.message
            ),
        }
    }
    let mut validation = world.resource_mut::<LevelValidation>();
    validation.problems = problems;
    validation.generation += 1;
    if demanded {
        let (errors, warnings) = {
            let validation = world.resource::<LevelValidation>();
            (
                validation.count(Severity::Error),
                validation.count(Severity::Warning),
            )
        };
        let message = if errors == 0 && warnings == 0 {
            "level valid \u{2713}".to_string()
        } else {
            format!("level: {errors} error(s), {warnings} warning(s) — see log")
        };
        world.write_message(super::SceneIoFeedback {
            message,
            success: errors == 0,
        });
    }
}

/// Editor builtins: the reference-breaking cases every game shares.
pub(crate) fn builtin_level_validators() -> Vec<LevelValidatorDef> {
    vec![
        LevelValidatorDef {
            id: ValidatorId::new_static("level.material-refs"),
            name: "Material references resolve",
            validate: |world| {
                let library_ids: Vec<uuid::Uuid> = world
                    .resource::<crate::materials::MaterialLibrary>()
                    .materials
                    .iter()
                    .map(|def| def.id)
                    .collect();
                let mut problems = Vec::new();
                let mut query =
                    world.query::<(&SceneId, &crate::materials::MaterialRef, Option<&Name>)>();
                for (id, material_ref, name) in query.iter(world) {
                    if !library_ids.contains(&material_ref.0) {
                        problems.push(LevelProblem {
                            validator: ValidatorId::new_static("level.material-refs"),
                            severity: Severity::Error,
                            message: format!(
                                "{:?} references a material missing from the library",
                                name.map(|n| n.as_str()).unwrap_or("entity")
                            ),
                            entity: Some(*id),
                        });
                    }
                }
                problems
            },
        },
        LevelValidatorDef {
            id: ValidatorId::new_static("level.model-refs"),
            name: "Model references resolve",
            validate: |world| {
                let known: Vec<uuid::Uuid> = world
                    .resource::<crate::models::ModelLibrary>()
                    .entries
                    .iter()
                    .map(|entry| entry.uuid)
                    .collect();
                let mut problems = Vec::new();
                let mut query = world.query::<(&SceneId, &crate::models::MeshRef, Option<&Name>)>();
                for (id, mesh_ref, name) in query.iter(world) {
                    if !known.contains(&mesh_ref.0) {
                        problems.push(LevelProblem {
                            validator: ValidatorId::new_static("level.model-refs"),
                            severity: Severity::Error,
                            message: format!(
                                "{:?} references an un-imported model",
                                name.map(|n| n.as_str()).unwrap_or("entity")
                            ),
                            entity: Some(*id),
                        });
                    }
                }
                problems
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    // v1-parity contract: builtins catch dangling references; a clean world
    // reports nothing.
    #[test]
    fn builtins_catch_dangling_references() {
        let mut world = World::new();
        world.init_resource::<crate::materials::MaterialLibrary>();
        world.init_resource::<crate::models::ModelLibrary>();

        let clean: usize = builtin_level_validators()
            .iter()
            .map(|v| (v.validate)(&mut world).len())
            .sum();
        assert_eq!(clean, 0, "empty world validates clean");

        world.spawn((
            SceneId::random(),
            Name::new("ghost"),
            crate::materials::MaterialRef(uuid::Uuid::new_v4()),
        ));
        world.spawn((
            SceneId::random(),
            crate::models::MeshRef(uuid::Uuid::new_v4()),
        ));

        let problems: Vec<LevelProblem> = builtin_level_validators()
            .iter()
            .flat_map(|v| (v.validate)(&mut world))
            .collect();
        assert_eq!(problems.len(), 2, "{problems:?}");
        assert!(problems.iter().all(|p| p.severity == Severity::Error));
        assert!(problems.iter().all(|p| p.entity.is_some()));
    }
}
