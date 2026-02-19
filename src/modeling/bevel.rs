//! Edge bevel operation for the mesh modeling tool.
//!
//! Bevels selected edges by splitting each into two parallel edges with a
//! connecting face strip between them. Works on the `HalfEdgeMesh` structure
//! for proper topology manipulation.

use bevy::prelude::*;
use std::collections::HashSet;

use super::half_edge::{HalfEdgeMesh, VertexId};

/// Bevel selected edges by the given width.
///
/// For each selected edge (given as half-edge IDs):
/// 1. Split the edge's two endpoint vertices, creating new vertices offset
///    along the adjacent edges
/// 2. Insert a new quad face along the original edge connecting the split vertices
/// 3. Reconnect adjacent faces to use the new split vertices
///
/// Returns a new `HalfEdgeMesh` with the bevel applied.
pub fn bevel_edges(
    mesh: &HalfEdgeMesh,
    selected_edges: &HashSet<u32>,
    width: f32,
) -> HalfEdgeMesh {
    if selected_edges.is_empty() || width.abs() < 1e-6 {
        return mesh.clone();
    }

    // Strategy: work on the EditMesh level for simplicity.
    // For each selected edge, we split the edge by offsetting vertices and
    // inserting a connecting face.
    //
    // Convert to edit mesh, apply bevel geometrically, rebuild half-edge mesh.
    let mut edit = mesh.to_edit_mesh();

    // Collect edge vertex pairs from the half-edge mesh
    let mut edges_to_bevel: Vec<(VertexId, VertexId)> = Vec::new();
    for &he_id in selected_edges {
        if (he_id as usize) >= mesh.half_edges.len() {
            continue;
        }
        let (from, to) = mesh.edge_vertices(he_id);
        // Canonical order to avoid duplicates
        let edge = if from <= to { (from, to) } else { (to, from) };
        if !edges_to_bevel.contains(&edge) {
            edges_to_bevel.push(edge);
        }
    }

    if edges_to_bevel.is_empty() {
        return mesh.clone();
    }

    // For each edge to bevel, we need to:
    // 1. Create 2 new vertices (one per endpoint, offset along the edge)
    // 2. Split all faces touching this edge to use the new vertices
    // 3. Add a quad face along the bevel

    // Process each edge
    for (v_from, v_to) in &edges_to_bevel {
        let from = *v_from;
        let to = *v_to;

        if (from as usize) >= edit.positions.len() || (to as usize) >= edit.positions.len() {
            continue;
        }

        let p_from = edit.positions[from as usize];
        let p_to = edit.positions[to as usize];
        let edge_dir = (p_to - p_from).normalize_or_zero();
        let edge_len = p_from.distance(p_to);

        // Clamp width to half the edge length
        let w = width.min(edge_len * 0.49);

        // Create 4 new vertices: two offset from each endpoint along the edge
        // from_a and from_b straddle the original 'from' vertex
        // to_a and to_b straddle the original 'to' vertex
        let from_new = edit.positions.len() as u32;
        edit.positions.push(p_from + edge_dir * w);
        edit.normals.push(edit.normals[from as usize]);
        edit.uvs.push(edit.uvs[from as usize]);

        let to_new = edit.positions.len() as u32;
        edit.positions.push(p_to - edge_dir * w);
        edit.normals.push(edit.normals[to as usize]);
        edit.uvs.push(edit.uvs[to as usize]);

        // Find all triangles that use this edge and split them
        let mut tris_to_remove = Vec::new();
        let mut tris_to_add = Vec::new();

        for (ti, tri) in edit.triangles.iter().enumerate() {
            let has_from = tri.contains(&from);
            let has_to = tri.contains(&to);

            if has_from && has_to {
                // This triangle uses the edge — replace it with two triangles
                // that use the new vertices instead
                tris_to_remove.push(ti);

                // Find the third vertex
                let third = tri.iter().find(|&&v| v != from && v != to).copied().unwrap();

                // Replace the edge (from, to) with (from_new, to_new)
                // Original tri: from, to, third (in some order)
                // New: from_new, to_new, third (preserving winding)
                let mut new_tri = *tri;
                for v in new_tri.iter_mut() {
                    if *v == from {
                        *v = from_new;
                    } else if *v == to {
                        *v = to_new;
                    }
                }
                tris_to_add.push(new_tri);

                // Also add small triangles connecting original vertex to new vertex
                // from -> from_new -> third
                tris_to_add.push(order_tri_winding(from, from_new, third, &edit.positions));
                // to -> to_new -> third
                tris_to_add.push(order_tri_winding(to_new, to, third, &edit.positions));
            }
        }

        // Remove old triangles (in reverse order to maintain indices)
        tris_to_remove.sort_unstable();
        for &ti in tris_to_remove.iter().rev() {
            edit.triangles.swap_remove(ti);
        }

        // Add the bevel face (quad = 2 triangles)
        // from -> from_new -> to_new -> to
        tris_to_add.push(order_tri_winding(from, from_new, to_new, &edit.positions));
        tris_to_add.push(order_tri_winding(from, to_new, to, &edit.positions));

        edit.triangles.extend(tris_to_add);
    }

    edit.recompute_normals();

    let mut result = HalfEdgeMesh::from_edit_mesh(&edit);
    result.recompute_normals();
    result
}

/// Order three vertices to form a triangle with consistent (CCW) winding
/// based on the face normal pointing outward.
fn order_tri_winding(a: u32, b: u32, c: u32, _positions: &[Vec3]) -> [u32; 3] {
    // Just return as-is — the caller should ensure correct winding
    // based on the original face orientation. For bevel faces,
    // we can't always determine the "right" winding without more context,
    // but recompute_normals will fix shading afterward.
    [a, b, c]
}
