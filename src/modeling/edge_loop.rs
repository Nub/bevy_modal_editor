//! Edge loop selection and insertion for the mesh modeling tool.
//!
//! Edge loops are rings of edges that follow a continuous path across quads
//! (or through triangulated quad pairs). Selection traverses perpendicular
//! edges via half-edge `next`/`twin`. Insertion splits crossed faces with
//! new edges.

use bevy::prelude::*;
use std::collections::HashSet;

use super::half_edge::{HalfEdgeMesh, HalfEdgeId, INVALID};

/// Select an edge loop starting from the given half-edge.
///
/// Walks perpendicular to the starting edge through adjacent faces. In a
/// triangle mesh, "perpendicular" means: from a half-edge, go to the opposite
/// edge of the triangle (the edge not sharing a vertex with the current one),
/// then cross to the twin face and repeat.
///
/// For true quads (pairs of triangles sharing a diagonal), this traces the
/// expected loop. For irregular topology it stops at boundaries or valence != 4.
pub fn select_edge_loop(mesh: &HalfEdgeMesh, start_he: HalfEdgeId) -> Vec<HalfEdgeId> {
    let mut loop_edges = Vec::new();
    let mut visited = HashSet::new();

    // Walk in both directions from the starting edge
    for &initial_dir in &[true, false] {
        let mut current = start_he;
        if !initial_dir {
            // Start from twin to walk the other direction
            let twin = mesh.half_edges[start_he as usize].twin;
            if twin == INVALID {
                continue;
            }
            current = twin;
        }

        loop {
            if !visited.insert(canonical_edge(mesh, current)) {
                break;
            }
            loop_edges.push(canonical_edge_he(mesh, current));

            // Traverse: from current half-edge, find the "opposite" edge in the face
            let next_he = next_loop_edge(mesh, current);
            if next_he == INVALID {
                break;
            }
            current = next_he;
        }
    }

    loop_edges.sort_unstable();
    loop_edges.dedup();
    loop_edges
}

/// Get the canonical half-edge ID for an edge (lower of he and twin).
fn canonical_edge(mesh: &HalfEdgeMesh, he: HalfEdgeId) -> HalfEdgeId {
    let twin = mesh.half_edges[he as usize].twin;
    if twin != INVALID && twin < he {
        twin
    } else {
        he
    }
}

/// Same as canonical_edge but always returns the interior (non-boundary) side.
fn canonical_edge_he(mesh: &HalfEdgeMesh, he: HalfEdgeId) -> HalfEdgeId {
    let twin = mesh.half_edges[he as usize].twin;
    if twin != INVALID && mesh.half_edges[he as usize].face == INVALID {
        twin
    } else if twin != INVALID && twin < he {
        twin
    } else {
        he
    }
}

/// Traverse from a half-edge to the next edge in the loop direction.
///
/// For a triangle face: the current half-edge enters the face. The "opposite"
/// edge is the one across from the shared vertex — i.e., `next.next` of the
/// current half-edge. Then cross to its twin face.
fn next_loop_edge(mesh: &HalfEdgeMesh, he: HalfEdgeId) -> HalfEdgeId {
    let face = mesh.half_edges[he as usize].face;
    if face == INVALID {
        return INVALID;
    }

    // In a triangle: he -> next -> next gives the opposite edge
    let n1 = mesh.half_edges[he as usize].next;
    if n1 == INVALID {
        return INVALID;
    }
    let opposite = mesh.half_edges[n1 as usize].next;
    if opposite == INVALID {
        return INVALID;
    }

    // Cross to twin face
    let twin = mesh.half_edges[opposite as usize].twin;
    if twin == INVALID {
        return INVALID;
    }

    // The twin's face is the next face in the loop
    let twin_face = mesh.half_edges[twin as usize].face;
    if twin_face == INVALID {
        return INVALID;
    }

    // In the twin face, continue across: twin -> next -> next
    let tn1 = mesh.half_edges[twin as usize].next;
    if tn1 == INVALID {
        return INVALID;
    }
    let next_edge = mesh.half_edges[tn1 as usize].next;
    if next_edge == INVALID {
        return INVALID;
    }

    // Cross to that edge's twin to continue the loop
    let next_twin = mesh.half_edges[next_edge as usize].twin;
    if next_twin == INVALID {
        return next_edge; // boundary — stop here
    }

    next_twin
}

/// Insert an edge loop by splitting all edges perpendicular to a given edge.
///
/// For each face crossed by the loop, a new vertex is inserted at the midpoint
/// of the crossed edge, and the face is split into two triangles through the
/// new vertex.
///
/// Returns the modified mesh.
pub fn insert_edge_loop(mesh: &HalfEdgeMesh, start_he: HalfEdgeId) -> HalfEdgeMesh {
    let loop_edges = select_edge_loop(mesh, start_he);
    if loop_edges.is_empty() {
        return mesh.clone();
    }

    // Work on edit mesh for simplicity
    let mut edit = mesh.to_edit_mesh();

    // Collect edges to split (as vertex pairs) and create midpoint vertices
    let mut edge_midpoints: Vec<(u32, u32, u32)> = Vec::new(); // (from, to, new_vertex_id)
    let mut processed_edges: HashSet<(u32, u32)> = HashSet::new();

    for &he_id in &loop_edges {
        if (he_id as usize) >= mesh.half_edges.len() {
            continue;
        }
        let (from, to) = mesh.edge_vertices(he_id);
        let key = if from <= to { (from, to) } else { (to, from) };
        if !processed_edges.insert(key) {
            continue;
        }

        let p_from = edit.positions[from as usize];
        let p_to = edit.positions[to as usize];
        let mid_pos = (p_from + p_to) * 0.5;
        let mid_normal = (edit.normals[from as usize] + edit.normals[to as usize])
            .normalize_or_zero();
        let mid_uv = (edit.uvs[from as usize] + edit.uvs[to as usize]) * 0.5;

        let new_id = edit.positions.len() as u32;
        edit.positions.push(mid_pos);
        edit.normals.push(mid_normal);
        edit.uvs.push(mid_uv);

        edge_midpoints.push((key.0, key.1, new_id));
    }

    // Split all triangles that contain a split edge
    let mut new_triangles = Vec::new();
    for tri in &edit.triangles {
        let mut splits: Vec<(usize, usize, u32)> = Vec::new(); // (idx_a, idx_b, midpoint_id)

        for &(from, to, mid) in &edge_midpoints {
            // Check if this triangle contains this edge
            let positions_in_tri: Vec<usize> = tri
                .iter()
                .enumerate()
                .filter(|(_, v)| **v == from || **v == to)
                .map(|(i, _)| i)
                .collect();

            if positions_in_tri.len() == 2 {
                splits.push((positions_in_tri[0], positions_in_tri[1], mid));
            }
        }

        if splits.is_empty() {
            new_triangles.push(*tri);
        } else if splits.len() == 1 {
            // One edge split: triangle becomes 2 triangles
            let (ia, ib, mid) = splits[0];
            let ic = 3 - ia - ib; // the other index
            let va = tri[ia];
            let vb = tri[ib];
            let vc = tri[ic];

            // Split: (va, mid, vc) and (mid, vb, vc)
            // Preserve winding by checking original order
            if (ia + 1) % 3 == ib {
                // a->b is forward in winding
                new_triangles.push([va, mid, vc]);
                new_triangles.push([mid, vb, vc]);
            } else {
                // b->a is forward
                new_triangles.push([vb, mid, vc]);
                new_triangles.push([mid, va, vc]);
            }
        } else {
            // Multiple splits on one triangle — rare, just keep original
            new_triangles.push(*tri);
        }
    }

    edit.triangles = new_triangles;
    edit.recompute_normals();

    let mut result = HalfEdgeMesh::from_edit_mesh(&edit);
    result.recompute_normals();
    result
}
