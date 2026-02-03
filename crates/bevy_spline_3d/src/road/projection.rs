//! Surface projection for road meshes.

use avian3d::prelude::*;
use bevy::{
    camera::primitives::Aabb,
    mesh::VertexAttributeValues,
    prelude::*,
};

use crate::surface::{create_projection_filter, SplineMeshProjection};

use super::{GeneratedRoadMesh, SplineRoad};

/// Run condition that checks if avian3d physics is available.
/// We check for the Gravity resource which is always present when PhysicsPlugins is added.
pub fn physics_available(gravity: Option<Res<Gravity>>) -> bool {
    gravity.is_some()
}

/// Marker component to track when a road mesh needs projection.
/// Added by mesh generation, removed after projection is applied.
#[derive(Component, Debug, Clone, Copy)]
pub struct NeedsProjection;

/// System to project road mesh vertices onto surfaces below.
pub fn project_road_meshes(
    mut commands: Commands,
    spatial_query: SpatialQuery,
    mut meshes: ResMut<Assets<Mesh>>,
    roads: Query<&SplineMeshProjection, With<SplineRoad>>,
    road_meshes: Query<(Entity, &GeneratedRoadMesh, &Mesh3d, &GlobalTransform), With<NeedsProjection>>,
) {
    for (mesh_entity, generated, mesh3d, global_transform) in &road_meshes {
        let Ok(config) = roads.get(generated.road) else {
            commands.entity(mesh_entity).remove::<NeedsProjection>();
            continue;
        };

        if !config.enabled {
            commands.entity(mesh_entity).remove::<NeedsProjection>();
            continue;
        }

        let Some(mesh) = meshes.get_mut(&mesh3d.0) else {
            continue;
        };

        // Get the world transform to convert local vertices to world space
        let transform = global_transform.compute_transform();
        let result = project_mesh_vertices(mesh, &spatial_query, config, &transform);

        // If we got hits, projection succeeded - update AABB and remove marker
        // If no hits, physics might not be ready yet - keep marker to retry next frame
        if let Some(aabb) = result {
            // Update the AABB component to reflect new mesh bounds
            // This is required for correct frustum culling and shadow maps
            commands.entity(mesh_entity)
                .insert(aabb)
                .remove::<NeedsProjection>();
        }
    }
}

/// Projection data for a single row (cross-section).
struct RowProjection {
    /// Offset to apply to all vertices in this row.
    offset: Vec3,
    /// Rotation to apply for terrain camber.
    rotation: Quat,
    /// Whether this row had a successful hit.
    has_hit: bool,
}

/// Project mesh vertices onto surfaces while preserving cross-section profiles.
///
/// Instead of projecting each vertex individually (which flattens the profile),
/// this function:
/// 1. Groups vertices into rows using UV V-coordinates (all vertices in a cross-section share the same V)
/// 2. For each row, finds the center point at the base (minimum Y)
/// 3. Projects only the center point to the terrain
/// 4. Smooths projection data across adjacent rows to avoid bumps
/// 5. Rotates the cross-section to match terrain slope (camber)
/// 6. Applies offset to all vertices in the row, preserving their relative positions
///
/// Returns the new AABB if any vertices were projected, None otherwise.
fn project_mesh_vertices(
    mesh: &mut Mesh,
    spatial_query: &SpatialQuery,
    config: &SplineMeshProjection,
    transform: &Transform,
) -> Option<Aabb> {
    let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION)?;
    let VertexAttributeValues::Float32x3(positions) = positions else {
        return None;
    };

    // Get UV coordinates to group vertices by row (same V = same cross-section)
    let uvs = mesh.attribute(Mesh::ATTRIBUTE_UV_0);
    let uvs: Option<&Vec<[f32; 2]>> = uvs.and_then(|v| {
        if let VertexAttributeValues::Float32x2(uvs) = v {
            Some(uvs)
        } else {
            None
        }
    });

    // Group vertices into rows by UV V-coordinate
    let rows = group_vertices_by_uv_row(positions, uvs);

    if rows.is_empty() {
        return None;
    }

    let filter = create_projection_filter(config);

    // Compute inverse transform for converting world -> local
    let inverse_affine = transform.compute_affine().inverse();

    // First pass: compute world-space centers for each row
    let row_centers: Vec<Vec3> = rows
        .iter()
        .map(|row_indices| {
            let row_positions: Vec<Vec3> = row_indices
                .iter()
                .map(|&i| Vec3::from_array(positions[i]))
                .collect();
            let local_center = compute_row_base_center(&row_positions);
            transform.transform_point(local_center)
        })
        .collect();

    // Second pass: collect raw projection data for each row
    let mut raw_projections: Vec<RowProjection> = Vec::with_capacity(rows.len());

    for (row_idx, _) in rows.iter().enumerate() {
        let world_center = row_centers[row_idx];
        let tangent = estimate_tangent(&row_centers, row_idx);

        let ray_origin = world_center + Vec3::Y * config.ray_origin_offset;
        let ray_direction = Dir3::NEG_Y;

        let projection = if let Some(hit) = spatial_query.cast_ray(
            ray_origin,
            ray_direction,
            config.max_distance,
            true,
            &filter,
        ) {
            let hit_position = ray_origin + *ray_direction * hit.distance;
            let world_adjusted = hit_position + hit.normal * config.normal_offset;
            let offset = world_adjusted - world_center;
            let rotation = compute_camber_rotation(tangent, hit.normal);

            RowProjection {
                offset,
                rotation,
                has_hit: true,
            }
        } else {
            RowProjection {
                offset: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                has_hit: false,
            }
        };

        raw_projections.push(projection);
    }

    // Count hits
    let hits = raw_projections.iter().filter(|p| p.has_hit).count();
    if hits == 0 {
        return None;
    }

    // Third pass: smooth the projection data
    let smoothed_projections = smooth_projections(&raw_projections);

    // Fourth pass: apply smoothed projections to vertices
    let mut new_positions: Vec<[f32; 3]> = vec![[0.0; 3]; positions.len()];

    for (row_idx, row_indices) in rows.iter().enumerate() {
        if row_indices.is_empty() {
            continue;
        }

        let world_center = row_centers[row_idx];
        let proj = &smoothed_projections[row_idx];

        for &idx in row_indices {
            let local_point = Vec3::from_array(positions[idx]);
            let world_point = transform.transform_point(local_point);

            // Rotate around the row center to apply camber
            let relative = world_point - world_center;
            let rotated = proj.rotation * relative;
            let world_rotated = world_center + rotated;

            // Apply the translation offset
            let world_adjusted = world_rotated + proj.offset;

            // Convert back to local space
            let local_adjusted = inverse_affine.transform_point3(world_adjusted);
            new_positions[idx] = [local_adjusted.x, local_adjusted.y, local_adjusted.z];
        }
    }

    // Apply the new positions
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, new_positions.clone());

    // Recompute normals for smooth shading
    mesh.compute_normals();

    // Remove stale tangent data
    mesh.remove_attribute(Mesh::ATTRIBUTE_TANGENT);

    // Compute new AABB from modified positions
    Aabb::enclosing(new_positions.iter().map(|p| Vec3::from_array(*p)))
}

/// Smooth projection data across adjacent rows using a weighted moving average.
/// This prevents bumpy roads caused by small terrain variations.
fn smooth_projections(raw: &[RowProjection]) -> Vec<RowProjection> {
    if raw.is_empty() {
        return Vec::new();
    }

    // Smoothing window size (number of rows on each side to consider)
    // Larger values = smoother but less terrain-conforming
    let window = 3;

    let mut smoothed = Vec::with_capacity(raw.len());

    for i in 0..raw.len() {
        let start = i.saturating_sub(window);
        let end = (i + window + 1).min(raw.len());

        let mut total_weight = 0.0;
        let mut sum_offset = Vec3::ZERO;
        let mut sum_rotation = Vec4::ZERO; // Using Vec4 for quaternion averaging

        for j in start..end {
            if !raw[j].has_hit {
                continue;
            }

            // Gaussian-like weight based on distance from center
            let dist = (i as f32 - j as f32).abs();
            let weight = (-dist * dist / (window as f32)).exp();

            sum_offset += raw[j].offset * weight;

            // For quaternion averaging, accumulate as vec4
            let q = raw[j].rotation;
            // Handle quaternion sign to ensure proper averaging
            let q_vec = Vec4::new(q.x, q.y, q.z, q.w);
            let q_vec = if sum_rotation.dot(q_vec) < 0.0 { -q_vec } else { q_vec };
            sum_rotation += q_vec * weight;

            total_weight += weight;
        }

        let (offset, rotation) = if total_weight > 0.001 {
            let avg_offset = sum_offset / total_weight;
            let avg_rotation = Quat::from_xyzw(
                sum_rotation.x / total_weight,
                sum_rotation.y / total_weight,
                sum_rotation.z / total_weight,
                sum_rotation.w / total_weight,
            )
            .normalize();
            (avg_offset, avg_rotation)
        } else {
            (Vec3::ZERO, Quat::IDENTITY)
        };

        smoothed.push(RowProjection {
            offset,
            rotation,
            has_hit: raw[i].has_hit,
        });
    }

    smoothed
}

/// Estimate the tangent (forward direction) at a row by looking at adjacent rows.
fn estimate_tangent(row_centers: &[Vec3], row_idx: usize) -> Vec3 {
    if row_centers.len() < 2 {
        return Vec3::Z;
    }

    let tangent = if row_idx == 0 {
        // First row: use direction to next row
        row_centers[1] - row_centers[0]
    } else if row_idx >= row_centers.len() - 1 {
        // Last row: use direction from previous row
        row_centers[row_idx] - row_centers[row_idx - 1]
    } else {
        // Middle rows: average of forward and backward directions
        let forward = row_centers[row_idx + 1] - row_centers[row_idx];
        let backward = row_centers[row_idx] - row_centers[row_idx - 1];
        (forward + backward) * 0.5
    };

    tangent.normalize_or_zero()
}

/// Compute rotation to tilt the road cross-section to match terrain slope.
/// This creates camber by rotating around the tangent (forward) axis.
fn compute_camber_rotation(tangent: Vec3, terrain_normal: Vec3) -> Quat {
    if tangent.length_squared() < 0.001 {
        return Quat::IDENTITY;
    }

    // The road's original "up" is world Y
    let original_up = Vec3::Y;

    // Project terrain normal onto the plane perpendicular to the tangent
    // This gives us the "effective up" direction for the road at this point
    let normal_along_tangent = tangent * terrain_normal.dot(tangent);
    let effective_up = (terrain_normal - normal_along_tangent).normalize_or_zero();

    if effective_up.length_squared() < 0.001 {
        return Quat::IDENTITY;
    }

    // Calculate the rotation from original_up to effective_up around the tangent axis
    // First, project both vectors onto the plane perpendicular to tangent
    let original_projected = (original_up - tangent * original_up.dot(tangent)).normalize_or_zero();

    if original_projected.length_squared() < 0.001 {
        return Quat::IDENTITY;
    }

    // Calculate angle between the projected vectors
    let dot = original_projected.dot(effective_up).clamp(-1.0, 1.0);
    let angle = dot.acos();

    // Determine rotation direction using cross product
    let cross = original_projected.cross(effective_up);
    let sign = if cross.dot(tangent) >= 0.0 { 1.0 } else { -1.0 };

    // Create rotation around tangent axis
    Quat::from_axis_angle(tangent, angle * sign)
}

/// Group vertex indices by their UV V-coordinate (rows in the mesh).
/// Returns a Vec of Vec<usize> where each inner Vec contains indices of vertices in the same row.
fn group_vertices_by_uv_row(
    positions: &[[f32; 3]],
    uvs: Option<&Vec<[f32; 2]>>,
) -> Vec<Vec<usize>> {
    let Some(uvs) = uvs else {
        // No UVs - fall back to single-vertex rows (old behavior, will flatten)
        return positions.iter().enumerate().map(|(i, _)| vec![i]).collect();
    };

    if uvs.len() != positions.len() {
        return positions.iter().enumerate().map(|(i, _)| vec![i]).collect();
    }

    // Group indices by V coordinate with tolerance
    let tolerance = 0.0001;
    let mut rows: Vec<(f32, Vec<usize>)> = Vec::new();

    for (idx, uv) in uvs.iter().enumerate() {
        let v = uv[1];

        // Find existing row with matching V
        let found = rows.iter_mut().find(|(row_v, _)| (row_v - v).abs() < tolerance);

        if let Some((_, indices)) = found {
            indices.push(idx);
        } else {
            rows.push((v, vec![idx]));
        }
    }

    // Sort rows by V coordinate for consistent ordering
    rows.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    // Extract just the index vectors
    rows.into_iter().map(|(_, indices)| indices).collect()
}

/// Compute the base center point of a row of vertices.
/// Returns a point at (average_x, min_y, average_z) - using min_y as the base height
/// so we project from the road surface, not the top of curbs.
fn compute_row_base_center(vertices: &[Vec3]) -> Vec3 {
    if vertices.is_empty() {
        return Vec3::ZERO;
    }

    let mut sum = Vec3::ZERO;
    let mut min_y = f32::MAX;

    for v in vertices {
        sum += *v;
        min_y = min_y.min(v.y);
    }

    let n = vertices.len() as f32;
    Vec3::new(sum.x / n, min_y, sum.z / n)
}
