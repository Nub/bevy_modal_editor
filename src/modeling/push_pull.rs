//! Push/pull operation for the mesh modeling tool.
//!
//! Moves selected faces along their individual normals without creating
//! side walls (unlike extrude which creates connecting geometry).

use bevy::prelude::*;
use std::collections::HashSet;

use super::edit_mesh::{EditMesh, FaceIndex};

/// Push or pull selected faces along their individual face normals.
///
/// Each selected face's vertices are offset by `distance` along the face's
/// normal. Vertices shared between multiple selected faces are moved by the
/// average of their adjacent face normals. Vertices shared between selected
/// and unselected faces are duplicated to avoid tearing the unselected faces.
///
/// Unlike extrude, no side wall geometry is created.
pub fn push_pull_faces(
    mesh: &EditMesh,
    selected: &HashSet<FaceIndex>,
    distance: f32,
) -> EditMesh {
    if selected.is_empty() || distance.abs() < 1e-6 {
        return mesh.clone();
    }

    let mut new_positions = mesh.positions.clone();
    let mut new_normals = mesh.normals.clone();
    let mut new_uvs = mesh.uvs.clone();
    let mut new_triangles = mesh.triangles.clone();

    // Compute per-vertex offset: average of adjacent selected face normals
    let selected_verts = mesh.selected_vertices(selected);
    let boundary_verts = mesh.boundary_vertices(selected);

    // Accumulate face normal contributions per vertex
    let mut vert_offset: Vec<Vec3> = vec![Vec3::ZERO; mesh.positions.len()];
    let mut vert_count: Vec<u32> = vec![0; mesh.positions.len()];

    for &fi in selected {
        if fi >= mesh.triangles.len() {
            continue;
        }
        let normal = mesh.face_normal(fi);
        for &v in &mesh.triangles[fi] {
            vert_offset[v as usize] += normal * distance;
            vert_count[v as usize] += 1;
        }
    }

    // Average the offset for vertices shared by multiple selected faces
    for v in &selected_verts {
        let count = vert_count[*v as usize];
        if count > 1 {
            vert_offset[*v as usize] /= count as f32;
        }
    }

    // Boundary vertices (shared with unselected faces) need to be duplicated
    // to avoid distorting unselected faces
    let mut dup_map: std::collections::HashMap<u32, u32> = std::collections::HashMap::new();

    for &v in &boundary_verts {
        let new_idx = new_positions.len() as u32;
        new_positions.push(mesh.positions[v as usize] + vert_offset[v as usize]);
        new_normals.push(mesh.normals[v as usize]);
        new_uvs.push(mesh.uvs[v as usize]);
        dup_map.insert(v, new_idx);
    }

    // Move interior selected vertices in-place
    for &v in &selected_verts {
        if !boundary_verts.contains(&v) {
            new_positions[v as usize] += vert_offset[v as usize];
        }
    }

    // Remap selected face vertices: boundary verts point to duplicates
    for &fi in selected {
        if fi >= new_triangles.len() {
            continue;
        }
        let tri = &mut new_triangles[fi];
        for idx in tri.iter_mut() {
            if let Some(&dup) = dup_map.get(idx) {
                *idx = dup;
            }
        }
    }

    let mut result = EditMesh {
        positions: new_positions,
        normals: new_normals,
        uvs: new_uvs,
        triangles: new_triangles,
    };
    result.recompute_normals();
    result
}
