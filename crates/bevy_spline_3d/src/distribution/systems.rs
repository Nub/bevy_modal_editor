use avian3d::prelude::Collider;
use bevy::prelude::*;

use crate::geometry::CoordinateFrame;
use crate::spline::{ArcLengthTable, Spline};
use crate::surface::SplineMeshProjection;

use super::{
    DistributedInstance, DistributionOrientation, DistributionSource, DistributionSpacing,
    DistributionState, SplineDistribution,
};
use super::projection::NeedsInstanceProjection;

/// Number of samples used to compute arc length lookup table.
const ARC_LENGTH_SAMPLES: usize = 256;

/// Hide entities marked as distribution sources.
pub fn hide_source_entities(
    mut sources: Query<&mut Visibility, Added<DistributionSource>>,
) {
    for mut visibility in &mut sources {
        *visibility = Visibility::Hidden;
    }
}

/// Update distributed instances when distribution or spline changes.
#[allow(clippy::too_many_arguments)]
pub fn update_distributions(
    mut commands: Commands,
    distributions: Query<(Entity, &SplineDistribution, Option<&DistributionState>)>,
    splines: Query<(&Spline, &GlobalTransform)>,
    sources: Query<(
        Option<&Mesh3d>,
        Option<&MeshMaterial3d<StandardMaterial>>,
        Option<&Collider>,
        Option<&Children>,
    )>,
    mut instances: Query<(&mut Transform, &DistributedInstance)>,
    changed_splines: Query<Entity, Or<(Changed<Spline>, Changed<GlobalTransform>)>>,
    changed_distributions: Query<Entity, Changed<SplineDistribution>>,
    projection_query: Query<(), With<SplineMeshProjection>>,
) {
    // Collect changed spline entities for quick lookup
    let changed_spline_set: std::collections::HashSet<Entity> =
        changed_splines.iter().collect();
    let changed_dist_set: std::collections::HashSet<Entity> =
        changed_distributions.iter().collect();

    for (dist_entity, distribution, state) in &distributions {
        if !distribution.enabled {
            continue;
        }

        let Ok((spline, spline_transform)) = splines.get(distribution.spline) else {
            continue;
        };

        if !spline.is_valid() {
            continue;
        }

        // Check if we need to rebuild instances
        let needs_rebuild = state.is_none()
            || changed_dist_set.contains(&dist_entity)
            || state.as_ref().is_some_and(|s| {
                s.cached_count != distribution.count || s.cached_source != distribution.source
            });

        // Check if we need to update transforms
        let needs_transform_update =
            needs_rebuild || changed_spline_set.contains(&distribution.spline);

        // Compute t values based on spacing mode
        let t_values = match distribution.spacing {
            DistributionSpacing::Uniform => compute_uniform_t_values(spline, distribution.count),
            DistributionSpacing::Parametric => compute_parametric_t_values(distribution.count),
        };

        if needs_rebuild {
            // Despawn old instances
            if let Some(state) = state {
                for &instance_entity in &state.instances {
                    if let Ok(mut entity_commands) = commands.get_entity(instance_entity) {
                        entity_commands.despawn();
                    }
                }
            }

            // Spawn new instances
            let mut new_instances = Vec::with_capacity(distribution.count);

            // Get source components to clone
            let source_data = sources.get(distribution.source).ok();

            for (i, &t) in t_values.iter().enumerate() {
                let transform = calculate_transform(spline, spline_transform, t, distribution);

                let mut entity_commands = commands.spawn((
                    transform,
                    DistributedInstance {
                        distribution: dist_entity,
                        index: i,
                    },
                    Visibility::default(),
                ));

                // Clone visual and physics components from source
                if let Some((mesh, material, collider, _children)) = source_data {
                    if let Some(mesh) = mesh {
                        entity_commands.insert(mesh.clone());
                    }
                    if let Some(material) = material {
                        entity_commands.insert(material.clone());
                    }
                    if let Some(collider) = collider {
                        entity_commands.insert(collider.clone());
                    }
                }

                // Mark for surface projection if enabled
                if projection_query.get(dist_entity).is_ok() {
                    entity_commands.insert(NeedsInstanceProjection);
                }

                new_instances.push(entity_commands.id());
            }

            // Update state
            commands.entity(dist_entity).insert(DistributionState {
                instances: new_instances,
                cached_count: distribution.count,
                cached_source: distribution.source,
            });
        } else if needs_transform_update {
            // Just update transforms on existing instances
            if let Some(state) = state {
                for (i, &instance_entity) in state.instances.iter().enumerate() {
                    if let Ok((mut transform, _)) = instances.get_mut(instance_entity) {
                        let t = t_values.get(i).copied().unwrap_or(0.5);
                        *transform = calculate_transform(spline, spline_transform, t, distribution);

                        // Mark for surface projection if enabled
                        if projection_query.get(dist_entity).is_ok() {
                            commands.entity(instance_entity).insert(NeedsInstanceProjection);
                        }
                    }
                }
            }
        }
    }
}

/// Compute t values for uniform distribution.
fn compute_uniform_t_values(spline: &Spline, count: usize) -> Vec<f32> {
    let table = ArcLengthTable::compute(spline, ARC_LENGTH_SAMPLES);
    table.uniform_t_values(count)
}

/// Compute t values for parametric distribution.
fn compute_parametric_t_values(count: usize) -> Vec<f32> {
    if count == 0 {
        return Vec::new();
    }
    if count == 1 {
        return vec![0.5];
    }

    (0..count)
        .map(|i| i as f32 / (count - 1) as f32)
        .collect()
}

/// Calculate transform for a distributed instance at parameter t.
/// The transform is computed in world space using the spline's GlobalTransform.
fn calculate_transform(
    spline: &Spline,
    spline_transform: &GlobalTransform,
    t: f32,
    distribution: &SplineDistribution,
) -> Transform {
    // Get position in local spline space
    let local_position = spline.evaluate(t).unwrap_or(Vec3::ZERO);

    // Calculate local rotation based on orientation mode
    let local_rotation = match distribution.orientation {
        DistributionOrientation::PositionOnly => Quat::IDENTITY,
        DistributionOrientation::AlignToTangent { up } => {
            if let Some(tangent) = spline.evaluate_tangent(t) {
                let frame = CoordinateFrame::from_tangent_with_up(tangent, up);
                if frame.is_valid() {
                    frame.to_rotation()
                } else {
                    Quat::IDENTITY
                }
            } else {
                Quat::IDENTITY
            }
        }
    };

    // Apply offset in local space
    let offset = local_rotation * distribution.offset;
    let local_pos_with_offset = local_position + offset;

    // Transform to world space using the spline's transform
    let world_position = spline_transform.transform_point(local_pos_with_offset);
    let world_rotation = spline_transform.to_scale_rotation_translation().1 * local_rotation;

    Transform {
        translation: world_position,
        rotation: world_rotation,
        scale: Vec3::ONE,
    }
}

/// Cleanup instances when distribution is removed.
pub fn cleanup_distributions(
    mut commands: Commands,
    mut removed: RemovedComponents<SplineDistribution>,
    states: Query<&DistributionState>,
    instances: Query<(Entity, &DistributedInstance)>,
) {
    for removed_dist in removed.read() {
        // Try to get state for cleanup
        if let Ok(state) = states.get(removed_dist) {
            for &instance_entity in &state.instances {
                if let Ok(mut entity_commands) = commands.get_entity(instance_entity) {
                    entity_commands.despawn();
                }
            }
        }

        // Also cleanup any instances that reference this distribution
        // (in case state wasn't available)
        for (entity, instance) in &instances {
            if instance.distribution == removed_dist {
                commands.entity(entity).despawn();
            }
        }
    }
}
