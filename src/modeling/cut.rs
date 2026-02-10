//! Cut/split algorithm for the mesh modeling tool.
//!
//! Splits a mesh into two parts along the boundary of a face selection.
//! Boundary vertices are duplicated so the two parts are fully separated.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::edit_mesh::{EditMesh, FaceIndex};

/// Split the mesh into two parts: unselected faces (kept on original entity)
/// and selected faces (spawned as a new entity).
///
/// Boundary vertices are duplicated so the two meshes are independent.
/// Returns `(remaining, cut_out)`.
pub fn cut_faces(
    mesh: &EditMesh,
    selected: &HashSet<FaceIndex>,
) -> (EditMesh, EditMesh) {
    if selected.is_empty() {
        return (mesh.clone(), empty_mesh());
    }
    if selected.len() == mesh.triangles.len() {
        return (empty_mesh(), mesh.clone());
    }

    // Build separate meshes by remapping vertices
    let mut remaining = extract_faces(mesh, |fi| !selected.contains(&fi));
    let mut cut_out = extract_faces(mesh, |fi| selected.contains(&fi));

    remaining.recompute_normals();
    cut_out.recompute_normals();

    (remaining, cut_out)
}

/// Extract faces matching a predicate into a new compact mesh.
fn extract_faces(mesh: &EditMesh, include: impl Fn(FaceIndex) -> bool) -> EditMesh {
    let mut new_positions = Vec::new();
    let mut new_normals = Vec::new();
    let mut new_uvs = Vec::new();
    let mut new_triangles = Vec::new();
    let mut vertex_map: HashMap<u32, u32> = HashMap::new();

    for (fi, tri) in mesh.triangles.iter().enumerate() {
        if !include(fi) {
            continue;
        }

        let mut new_tri = [0u32; 3];
        for (i, &v) in tri.iter().enumerate() {
            let new_idx = *vertex_map.entry(v).or_insert_with(|| {
                let idx = new_positions.len() as u32;
                new_positions.push(mesh.positions[v as usize]);
                new_normals.push(mesh.normals[v as usize]);
                new_uvs.push(mesh.uvs[v as usize]);
                idx
            });
            new_tri[i] = new_idx;
        }
        new_triangles.push(new_tri);
    }

    EditMesh {
        positions: new_positions,
        normals: new_normals,
        uvs: new_uvs,
        triangles: new_triangles,
    }
}

/// Create an empty mesh with no geometry.
fn empty_mesh() -> EditMesh {
    EditMesh {
        positions: Vec::new(),
        normals: Vec::new(),
        uvs: Vec::new(),
        triangles: Vec::new(),
    }
}
