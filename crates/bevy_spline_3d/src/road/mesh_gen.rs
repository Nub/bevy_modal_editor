use bevy::{
    prelude::*,
    mesh::{Indices, PrimitiveTopology, VertexAttributeValues},
};

use crate::geometry::CoordinateFrame;
use crate::spline::Spline;
use crate::surface::SplineMeshProjection;

use super::{GeneratedRoadMesh, SplineRoad};
use super::projection::NeedsProjection;

/// Creates a simple road segment mesh for testing.
///
/// This generates a flat road surface with optional curbs.
///
/// # Arguments
/// * `width` - Total road width
/// * `segment_length` - Length of this segment (Z extent)
/// * `curb_height` - Height of curbs on each side (0 for no curbs)
/// * `curb_width` - Width of each curb
pub fn create_road_segment_mesh(
    width: f32,
    segment_length: f32,
    curb_height: f32,
    curb_width: f32,
) -> Mesh {
    let hw = width / 2.0; // half width
    let road_hw = hw - curb_width; // half width of road surface

    // Cross-section profile (from left to right):
    // [curb_left_outer, curb_left_inner, road_left, road_right, curb_right_inner, curb_right_outer]

    let profile = if curb_height > 0.0 && curb_width > 0.0 {
        vec![
            Vec3::new(-hw, curb_height, 0.0),          // Left curb outer top
            Vec3::new(-road_hw, curb_height, 0.0),     // Left curb inner top
            Vec3::new(-road_hw, 0.0, 0.0),             // Left road edge
            Vec3::new(road_hw, 0.0, 0.0),              // Right road edge
            Vec3::new(road_hw, curb_height, 0.0),      // Right curb inner top
            Vec3::new(hw, curb_height, 0.0),           // Right curb outer top
        ]
    } else {
        vec![
            Vec3::new(-hw, 0.0, 0.0),
            Vec3::new(hw, 0.0, 0.0),
        ]
    };

    let profile_len = profile.len();

    // Generate vertices for front (Z=0) and back (Z=segment_length) edges
    let mut positions = Vec::with_capacity(profile_len * 2);
    let mut normals = Vec::with_capacity(profile_len * 2);
    let mut uvs = Vec::with_capacity(profile_len * 2);

    // Front edge
    for (i, p) in profile.iter().enumerate() {
        positions.push([p.x, p.y, 0.0]);
        normals.push([0.0, 1.0, 0.0]); // Up-facing normal (simplified)
        uvs.push([i as f32 / (profile_len - 1) as f32, 0.0]);
    }

    // Back edge
    for (i, p) in profile.iter().enumerate() {
        positions.push([p.x, p.y, segment_length]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([i as f32 / (profile_len - 1) as f32, 1.0]);
    }

    // Generate indices (triangles between front and back edges)
    // Counter-clockwise winding for upward-facing normals
    let mut indices = Vec::new();
    for i in 0..(profile_len - 1) {
        let front_left = i as u32;
        let front_right = (i + 1) as u32;
        let back_left = (i + profile_len) as u32;
        let back_right = (i + 1 + profile_len) as u32;

        // Two triangles per quad (CW winding for upward-facing in Bevy)
        indices.extend_from_slice(&[front_left, front_right, back_left]);
        indices.extend_from_slice(&[front_right, back_right, back_left]);
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));

    mesh
}

/// A vertex in a mesh cross-section profile.
#[derive(Debug, Clone)]
pub struct ProfileVertex {
    /// The vertex position.
    pub position: Vec3,
    /// The UV coordinates (if available).
    pub uv: Option<Vec2>,
}

/// Extract the cross-section profile from a segment mesh.
///
/// Returns vertices at the front edge (minimum Z) sorted by X coordinate.
/// If `include_uvs` is true, UV coordinates are extracted when available.
pub fn extract_mesh_profile(mesh: &Mesh, include_uvs: bool) -> Option<Vec<ProfileVertex>> {
    let positions = mesh.attribute(Mesh::ATTRIBUTE_POSITION)?;
    let positions = match positions {
        VertexAttributeValues::Float32x3(v) => v,
        _ => return None,
    };

    let uvs: Option<&Vec<[f32; 2]>> = if include_uvs {
        mesh.attribute(Mesh::ATTRIBUTE_UV_0).and_then(|attr| {
            if let VertexAttributeValues::Float32x2(v) = attr {
                Some(v)
            } else {
                None
            }
        })
    } else {
        None
    };

    // Find the minimum Z value (front edge)
    let min_z = positions
        .iter()
        .map(|p| p[2])
        .min_by(|a: &f32, b: &f32| a.partial_cmp(b).unwrap())?;

    // Collect vertices at the front edge (within tolerance)
    let tolerance = 0.001;
    let mut profile: Vec<ProfileVertex> = positions
        .iter()
        .enumerate()
        .filter(|(_, p)| (p[2] - min_z).abs() < tolerance)
        .map(|(i, p)| ProfileVertex {
            position: Vec3::new(p[0], p[1], p[2]),
            uv: uvs.map(|uvs| Vec2::new(uvs[i][0], uvs[i][1])),
        })
        .collect();

    // Sort by X coordinate for consistent ordering
    profile.sort_by(|a, b| a.position.x.partial_cmp(&b.position.x).unwrap());

    Some(profile)
}

/// Generate a road mesh by extruding a cross-section along a spline.
pub fn generate_road_mesh(
    spline: &Spline,
    segment_mesh: &Mesh,
    segments: usize,
    uv_tile_length: f32,
) -> Option<Mesh> {
    let profile = extract_mesh_profile(segment_mesh, true)?;
    if profile.is_empty() {
        return None;
    }

    let profile_len = profile.len();
    let total_vertices = profile_len * (segments + 1);

    let mut positions = Vec::with_capacity(total_vertices);
    let mut normals = Vec::with_capacity(total_vertices);
    let mut uvs = Vec::with_capacity(total_vertices);

    // Sample spline at each segment point
    for seg_idx in 0..=segments {
        let t = seg_idx as f32 / segments as f32;

        let position = spline.evaluate(t)?;
        let tangent = spline
            .evaluate_tangent(t)
            .map(|t| t.normalize_or_zero())
            .unwrap_or(Vec3::Z);

        // Build local coordinate frame
        let frame = CoordinateFrame::from_tangent(tangent);

        // Transform each profile vertex
        for vertex in &profile {
            // Transform from local to world space using coordinate frame
            let world_offset = frame.transform_profile_point(vertex.position.x, vertex.position.y);
            let world_pos = position + world_offset;

            positions.push([world_pos.x, world_pos.y, world_pos.z]);
            normals.push([frame.up.x, frame.up.y, frame.up.z]);

            // UV: X from profile, Y from spline progress
            let v = t * uv_tile_length;
            let u = vertex.uv.map(|uv| uv.x).unwrap_or(0.0);
            uvs.push([u, v]);
        }
    }

    // Generate indices
    // CCW winding for upward-facing normals
    let mut indices = Vec::new();
    for seg_idx in 0..segments {
        let row_start = seg_idx * profile_len;
        let next_row_start = (seg_idx + 1) * profile_len;

        for i in 0..(profile_len - 1) {
            let a = (row_start + i) as u32;
            let b = (row_start + i + 1) as u32;
            let c = (next_row_start + i) as u32;
            let d = (next_row_start + i + 1) as u32;

            // Two triangles per quad (CW winding for upward-facing in Bevy)
            // a=back-left, b=back-right, c=front-left, d=front-right
            indices.extend_from_slice(&[a, b, c]);
            indices.extend_from_slice(&[b, d, c]);
        }
    }

    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));

    // Recompute normals for smooth shading
    mesh.compute_normals();

    Some(mesh)
}

/// System to update road meshes when splines change.
#[allow(clippy::too_many_arguments)]
pub fn update_road_meshes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    roads: Query<(Entity, &SplineRoad, Option<&MeshMaterial3d<StandardMaterial>>), Changed<SplineRoad>>,
    splines: Query<&Spline>,
    changed_splines: Query<Entity, Changed<Spline>>,
    all_roads: Query<(Entity, &SplineRoad, Option<&MeshMaterial3d<StandardMaterial>>)>,
    existing_road_meshes: Query<(Entity, &GeneratedRoadMesh)>,
    road_mesh_children: Query<&Children>,
    projection_query: Query<(), With<SplineMeshProjection>>,
) {
    let changed_spline_set: std::collections::HashSet<Entity> = changed_splines.iter().collect();

    // Collect roads that need updating
    let mut roads_to_update: Vec<(Entity, &SplineRoad, Option<&MeshMaterial3d<StandardMaterial>>)> = roads.iter().collect();

    // Also update roads whose splines changed
    for (entity, road, material) in &all_roads {
        if road.auto_update && changed_spline_set.contains(&road.spline) {
            if !roads_to_update.iter().any(|(e, _, _)| *e == entity) {
                roads_to_update.push((entity, road, material));
            }
        }
    }

    for (road_entity, road, material) in roads_to_update {
        let Ok(spline) = splines.get(road.spline) else {
            continue;
        };

        if !spline.is_valid() {
            continue;
        }

        let Some(segment_mesh) = meshes.get(&road.segment_mesh) else {
            continue;
        };

        let Some(generated) = generate_road_mesh(
            spline,
            segment_mesh,
            road.segments_per_curve,
            road.uv_tile_length,
        ) else {
            continue;
        };

        let mesh_handle = meshes.add(generated);

        // Find or create the mesh entity
        let mut found_mesh_entity = None;
        if let Ok(children) = road_mesh_children.get(road_entity) {
            for child in children.iter() {
                if existing_road_meshes.get(child).is_ok() {
                    found_mesh_entity = Some(child);
                    break;
                }
            }
        }

        if let Some(mesh_entity) = found_mesh_entity {
            // Update existing mesh
            let mut entity_commands = commands.entity(mesh_entity);
            entity_commands.insert(Mesh3d(mesh_handle));
            // Update material if present
            if let Some(mat) = material {
                entity_commands.insert(mat.clone());
            }
            // Mark for surface projection if enabled
            if projection_query.get(road_entity).is_ok() {
                entity_commands.insert(NeedsProjection);
            }
        } else {
            // Spawn new mesh entity as child
            let mut entity_commands = commands.spawn((
                Mesh3d(mesh_handle),
                Transform::default(),
                Visibility::default(),
                GeneratedRoadMesh { road: road_entity },
            ));

            // Copy material from parent
            if let Some(mat) = material {
                entity_commands.insert(mat.clone());
            }

            // Mark for surface projection if enabled
            if projection_query.get(road_entity).is_ok() {
                entity_commands.insert(NeedsProjection);
            }

            let mesh_entity = entity_commands.id();
            commands.entity(road_entity).add_child(mesh_entity);
        }
    }
}
