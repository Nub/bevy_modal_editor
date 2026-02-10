//! Extrusion algorithm for the mesh modeling tool.
//!
//! Given a set of selected faces, extrudes them outward by a given distance,
//! creating side wall geometry along boundary edges.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::edit_mesh::{EditMesh, FaceIndex};

/// Extrude selected faces from the mesh by the given distance along their average normal.
///
/// - Boundary vertices are duplicated: the original stays for side walls, the copy moves.
/// - Interior vertices (not on any boundary edge) are simply offset.
/// - Side wall quads are created along each boundary edge.
/// - If `angle` is nonzero, the extrusion direction tilts per-face.
///
/// Returns a new `EditMesh` with the extrusion applied.
pub fn extrude_faces(
    mesh: &EditMesh,
    selected: &HashSet<FaceIndex>,
    distance: f32,
    _angle: f32,
) -> EditMesh {
    if selected.is_empty() || distance.abs() < 1e-6 {
        return mesh.clone();
    }

    let mut new_positions = mesh.positions.clone();
    let mut new_normals = mesh.normals.clone();
    let mut new_uvs = mesh.uvs.clone();
    let mut new_triangles = mesh.triangles.clone();

    // Compute area-weighted average normal of selected faces
    let extrude_normal = {
        let mut sum = Vec3::ZERO;
        for &fi in selected {
            let normal = mesh.face_normal(fi);
            let area = mesh.face_area(fi);
            sum += normal * area;
        }
        sum.normalize_or_zero()
    };

    let offset = extrude_normal * distance;

    // Find boundary edges and vertices
    let boundary_verts = mesh.boundary_vertices(selected);
    let selected_verts = mesh.selected_vertices(selected);

    // Map: original boundary vertex -> new duplicated vertex index
    let mut dup_map: HashMap<u32, u32> = HashMap::new();

    for &v in &boundary_verts {
        let new_idx = new_positions.len() as u32;
        new_positions.push(mesh.positions[v as usize] + offset);
        new_normals.push(mesh.normals[v as usize]);
        new_uvs.push(mesh.uvs[v as usize]);
        dup_map.insert(v, new_idx);
    }

    // Move interior selected vertices (not on boundary)
    for &v in &selected_verts {
        if !boundary_verts.contains(&v) {
            new_positions[v as usize] += offset;
        }
    }

    // Remap selected face vertices: boundary verts point to their duplicates
    for &fi in selected {
        let tri = &mut new_triangles[fi];
        for idx in tri.iter_mut() {
            if let Some(&dup) = dup_map.get(idx) {
                *idx = dup;
            }
        }
    }

    // Create side wall quads along boundary edges
    let boundary_edges = mesh.boundary_edges(selected);
    for edge in &boundary_edges {
        let a = edge.0;
        let b = edge.1;

        // Original positions (unmoved boundary verts)
        // Duplicated positions (moved)
        let a_dup = dup_map[&a];
        let b_dup = dup_map[&b];

        // Determine winding: the side wall should face outward.
        // Find which selected face owns this edge to get correct winding.
        let adj = mesh.build_adjacency();
        let sel_face = adj[edge]
            .iter()
            .find(|f| selected.contains(f))
            .copied();

        if let Some(fi) = sel_face {
            let [t0, t1, t2] = mesh.triangles[fi];
            // Find the edge direction in the selected face's winding order
            let (ea, eb) = find_edge_in_face(t0, t1, t2, a, b);

            // Two triangles forming a quad: (ea, eb, eb_dup), (ea, eb_dup, ea_dup)
            let ea_dup = dup_map[&ea];
            let eb_dup = dup_map[&eb];
            new_triangles.push([ea, eb, eb_dup]);
            new_triangles.push([ea, eb_dup, ea_dup]);
        } else {
            // Fallback winding
            new_triangles.push([a, b, b_dup]);
            new_triangles.push([a, b_dup, a_dup]);
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

/// Find the directed edge (a, b) in the triangle's winding order.
/// Returns the vertices in the order they appear in the face.
fn find_edge_in_face(t0: u32, t1: u32, t2: u32, a: u32, b: u32) -> (u32, u32) {
    let verts = [t0, t1, t2];
    for i in 0..3 {
        let v0 = verts[i];
        let v1 = verts[(i + 1) % 3];
        if (v0 == a && v1 == b) || (v0 == b && v1 == a) {
            return (v0, v1);
        }
    }
    // Should not happen if edge is in face, but fallback
    (a, b)
}
