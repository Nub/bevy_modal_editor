//! Surface projection utilities.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::camera::primitives::Aabb;

use crate::placer::SurfaceProjection;

/// Result of a surface projection.
#[derive(Debug, Clone)]
pub struct ProjectionResult {
    /// Hit position on the surface.
    pub position: Vec3,
    /// Surface normal at hit point.
    pub normal: Vec3,
    /// Entity that was hit.
    pub entity: Entity,
}

/// Project a point onto a surface using raycasting.
///
/// # Arguments
/// * `spatial_query` - The spatial query resource for raycasting
/// * `origin` - Starting point for the raycast
/// * `config` - Projection configuration
/// * `source_rotation` - Optional rotation to transform local-space direction to world space
///
/// # Returns
/// The projection result if a surface was hit, None otherwise.
pub fn project_to_surface(
    spatial_query: &SpatialQuery,
    origin: Vec3,
    config: &SurfaceProjection,
    source_rotation: Option<Quat>,
) -> Option<ProjectionResult> {
    if !config.enabled {
        return None;
    }

    // Transform direction to world space if local_space is enabled
    let world_direction = if config.local_space {
        source_rotation.unwrap_or(Quat::IDENTITY) * config.direction
    } else {
        config.direction
    };

    let direction = Dir3::new(world_direction).ok()?;
    let ray_origin = origin - world_direction * config.ray_origin_offset;

    let mut filter = if let Some(layers) = config.collision_layers {
        SpatialQueryFilter::from_mask(layers)
    } else {
        SpatialQueryFilter::default()
    };

    // Exclude entities from raycast
    if !config.exclude_entities.is_empty() {
        filter = filter.with_excluded_entities(config.exclude_entities.iter().copied());
    }

    let hit = spatial_query.cast_ray(ray_origin, direction, config.max_distance, true, &filter)?;

    Some(ProjectionResult {
        position: ray_origin + world_direction * hit.distance,
        normal: hit.normal,
        entity: hit.entity,
    })
}

/// Calculate the offset needed to prevent an object from clipping through a surface.
///
/// Uses the collider's AABB to find the half-extent along the surface normal axis.
///
/// # Arguments
/// * `collider` - Optional collider to get bounds from
/// * `surface_normal` - The surface normal to offset along
///
/// # Returns
/// The offset distance to apply along the surface normal.
pub fn calculate_bounds_offset(collider: Option<&Collider>, surface_normal: Vec3) -> f32 {
    let Some(collider) = collider else {
        return 0.0;
    };

    let aabb = collider.aabb(Vec3::ZERO, Quat::IDENTITY);
    // ColliderAabb returns min/max, calculate half_extents from that
    let half_extents = (aabb.max - aabb.min) * 0.5;
    let normal_abs = surface_normal.abs();

    // Project half-extents onto the normal to get the offset
    half_extents.dot(normal_abs)
}

/// Calculate the offset using mesh bounds instead of collider.
pub fn calculate_mesh_bounds_offset(aabb: &Aabb, surface_normal: Vec3) -> f32 {
    let half_extents: Vec3 = aabb.half_extents.into();
    let normal_abs = surface_normal.abs();
    half_extents.dot(normal_abs)
}
