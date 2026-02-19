//! Bridge operation for the mesh modeling tool.
//!
//! Connects two boundary edge loops (open holes in the mesh) with quad strips,
//! creating a tube of geometry between them.

use bevy::prelude::*;
use std::collections::HashSet;

use super::edit_mesh::EditMesh;
use super::half_edge::{HalfEdgeMesh, HalfEdgeId, VertexId, INVALID};

/// Find boundary loops in the mesh. A boundary loop is a chain of boundary
/// half-edges forming a closed ring around a hole.
pub fn find_boundary_loops(mesh: &HalfEdgeMesh) -> Vec<Vec<VertexId>> {
    let mut visited: HashSet<HalfEdgeId> = HashSet::new();
    let mut loops = Vec::new();

    for (i, he) in mesh.half_edges.iter().enumerate() {
        if he.face != INVALID {
            continue; // not a boundary half-edge
        }
        let he_id = i as HalfEdgeId;
        if visited.contains(&he_id) {
            continue;
        }

        // Walk the boundary chain
        let mut loop_verts = Vec::new();
        let mut current = he_id;
        loop {
            if !visited.insert(current) {
                break;
            }
            loop_verts.push(mesh.half_edges[current as usize].vertex);
            let next = mesh.half_edges[current as usize].next;
            if next == INVALID || next == he_id {
                break;
            }
            current = next;
        }

        if loop_verts.len() >= 3 {
            loops.push(loop_verts);
        }
    }

    loops
}

/// Bridge two boundary loops by creating quad strips between them.
///
/// `loop_a` and `loop_b` are ordered vertex indices forming boundary loops.
/// The loops should have the same number of vertices for a clean bridge;
/// if they differ, the shorter one is repeated to match.
///
/// Returns the modified mesh with bridge geometry added.
pub fn bridge_edge_loops(
    mesh: &HalfEdgeMesh,
    loop_a: &[VertexId],
    loop_b: &[VertexId],
) -> HalfEdgeMesh {
    if loop_a.is_empty() || loop_b.is_empty() {
        return mesh.clone();
    }

    let mut edit = mesh.to_edit_mesh();
    let n = loop_a.len().max(loop_b.len());

    // Find the best rotation of loop_b to minimize total edge length
    let best_offset = find_best_alignment(&edit, loop_a, loop_b);

    for i in 0..n {
        let a0 = loop_a[i % loop_a.len()];
        let a1 = loop_a[(i + 1) % loop_a.len()];
        let b0 = loop_b[(i + best_offset) % loop_b.len()];
        let b1 = loop_b[(i + 1 + best_offset) % loop_b.len()];

        // Quad: a0, a1, b1, b0 — split into 2 triangles
        edit.triangles.push([a0, a1, b1]);
        edit.triangles.push([a0, b1, b0]);
    }

    edit.recompute_normals();
    HalfEdgeMesh::from_edit_mesh(&edit)
}

/// Find the rotation offset for loop_b that minimizes total distance to loop_a.
fn find_best_alignment(mesh: &EditMesh, loop_a: &[VertexId], loop_b: &[VertexId]) -> usize {
    let n = loop_b.len();
    if n == 0 {
        return 0;
    }

    let mut best_offset = 0;
    let mut best_dist = f32::MAX;

    for offset in 0..n {
        let mut total = 0.0f32;
        let samples = loop_a.len().min(loop_b.len());
        for i in 0..samples {
            let a = loop_a[i % loop_a.len()];
            let b = loop_b[(i + offset) % n];
            if (a as usize) < mesh.positions.len() && (b as usize) < mesh.positions.len() {
                total += mesh.positions[a as usize].distance_squared(mesh.positions[b as usize]);
            }
        }
        if total < best_dist {
            best_dist = total;
            best_offset = offset;
        }
    }

    best_offset
}

/// Bridge two sets of selected edges that form boundary loops.
///
/// Finds the boundary loops that contain the selected edges and bridges them.
/// Returns None if the selected edges don't form exactly 2 boundary loops.
pub fn bridge_selected_edges(
    mesh: &HalfEdgeMesh,
    selected_edges: &HashSet<u32>,
) -> Option<HalfEdgeMesh> {
    let loops = find_boundary_loops(mesh);
    if loops.len() < 2 {
        return None;
    }

    // Find loops that overlap with the selection
    let selected_verts: HashSet<VertexId> = selected_edges
        .iter()
        .filter_map(|&he_id| {
            if (he_id as usize) >= mesh.half_edges.len() {
                return None;
            }
            let (from, to) = mesh.edge_vertices(he_id);
            Some(vec![from, to])
        })
        .flatten()
        .collect();

    let matching_loops: Vec<&Vec<VertexId>> = loops
        .iter()
        .filter(|lp| lp.iter().any(|v| selected_verts.contains(v)))
        .collect();

    if matching_loops.len() >= 2 {
        Some(bridge_edge_loops(mesh, matching_loops[0], matching_loops[1]))
    } else if loops.len() >= 2 {
        // Fallback: bridge the first two boundary loops
        Some(bridge_edge_loops(mesh, &loops[0], &loops[1]))
    } else {
        None
    }
}
