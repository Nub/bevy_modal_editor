//! Surface projection for distributed instances.

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::surface::{cast_projection_ray, SplineMeshProjection};

use super::{DistributedInstance, SplineDistribution};

/// Run condition that checks if avian3d physics is available.
/// We check for the Gravity resource which is always present when PhysicsPlugins is added.
pub fn physics_available(gravity: Option<Res<Gravity>>) -> bool {
    gravity.is_some()
}

/// Marker component for instances that need projection.
/// Added when instances are created/updated, removed after projection.
#[derive(Component, Debug, Clone, Copy)]
pub struct NeedsInstanceProjection;

/// System to project distributed instances onto surfaces below.
pub fn project_distributed_instances(
    mut commands: Commands,
    spatial_query: SpatialQuery,
    distributions: Query<&SplineMeshProjection, With<SplineDistribution>>,
    mut instances: Query<(Entity, &DistributedInstance, &mut Transform), With<NeedsInstanceProjection>>,
) {
    for (instance_entity, instance, mut transform) in &mut instances {
        let Ok(config) = distributions.get(instance.distribution) else {
            commands.entity(instance_entity).remove::<NeedsInstanceProjection>();
            continue;
        };

        if !config.enabled {
            commands.entity(instance_entity).remove::<NeedsInstanceProjection>();
            continue;
        }

        if let Some(hit) = cast_projection_ray(&spatial_query, transform.translation, config) {
            transform.translation = hit.with_normal_offset(config.normal_offset);

            // Optionally align rotation to surface normal
            if config.align_to_normal {
                let normal = hit.normal;
                let forward = transform.forward();
                let right = normal.cross(*forward).normalize_or_zero();
                if right.length_squared() > 0.001 {
                    let corrected_forward = right.cross(normal).normalize();
                    transform.rotation =
                        Quat::from_mat3(&Mat3::from_cols(right, normal, corrected_forward));
                }
            }

            // Projection succeeded - remove marker
            commands.entity(instance_entity).remove::<NeedsInstanceProjection>();
        }
        // If no hit, keep marker to retry next frame (physics might not be ready)
    }
}
