//! Element deletion for the mesh modeling tool.
//!
//! Deletes selected elements from the mesh:
//! - Face deletion: removes selected triangles
//! - Edge dissolution: merges the two faces adjacent to each selected edge
//! - Vertex dissolution: removes vertex and retriangulates the hole

use bevy::prelude::*;
use std::collections::HashSet;

use super::edit_mesh::{EditMesh, FaceIndex};
use super::half_edge::{HalfEdgeMesh, HalfEdgeId, VertexId, INVALID};

/// Delete selected faces from the mesh.
///
/// Simply removes the triangles. Leaves the vertices in place (they become
/// orphaned but don't affect rendering). Recomputes normals.
pub fn delete_faces(mesh: &EditMesh, selected: &HashSet<FaceIndex>) -> EditMesh {
    if selected.is_empty() {
        return mesh.clone();
    }

    let new_triangles: Vec<[u32; 3]> = mesh
        .triangles
        .iter()
        .enumerate()
        .filter(|(fi, _)| !selected.contains(fi))
        .map(|(_, tri)| *tri)
        .collect();

    let mut result = EditMesh {
        positions: mesh.positions.clone(),
        normals: mesh.normals.clone(),
        uvs: mesh.uvs.clone(),
        triangles: new_triangles,
    };
    result.recompute_normals();
    result
}

/// Dissolve selected edges: merge adjacent face pairs across each selected edge.
///
/// For each selected edge with two adjacent triangles, the two triangles are
/// replaced by a quad (2 new triangles) that skips the dissolved edge.
/// Boundary edges (only one adjacent face) are simply deleted along with their face.
pub fn dissolve_edges(
    mesh: &HalfEdgeMesh,
    selected_edges: &HashSet<HalfEdgeId>,
) -> HalfEdgeMesh {
    if selected_edges.is_empty() {
        return mesh.clone();
    }

    let mut edit = mesh.to_edit_mesh();

    // Collect face pairs to merge
    let mut faces_to_remove: HashSet<usize> = HashSet::new();
    let mut new_tris: Vec<[u32; 3]> = Vec::new();

    for &he_id in selected_edges {
        if (he_id as usize) >= mesh.half_edges.len() {
            continue;
        }

        let he = &mesh.half_edges[he_id as usize];
        let twin_id = he.twin;
        if twin_id == INVALID {
            // Boundary edge — just remove the adjacent face
            if he.face != INVALID {
                faces_to_remove.insert(he.face as usize);
            }
            continue;
        }

        let twin = &mesh.half_edges[twin_id as usize];
        let face_a = he.face;
        let face_b = twin.face;

        if face_a == INVALID || face_b == INVALID {
            // One side is boundary — remove the interior face
            if face_a != INVALID {
                faces_to_remove.insert(face_a as usize);
            }
            if face_b != INVALID {
                faces_to_remove.insert(face_b as usize);
            }
            continue;
        }

        // Both faces valid — dissolve the edge
        // Get the 4 unique vertices of the two triangles
        let verts_a = mesh.face_vertices(face_a);
        let verts_b = mesh.face_vertices(face_b);

        let (from, to) = mesh.edge_vertices(he_id);

        // Find the two vertices NOT on the dissolved edge
        let apex_a = verts_a.iter().find(|&&v| v != from && v != to).copied();
        let apex_b = verts_b.iter().find(|&&v| v != from && v != to).copied();

        if let (Some(a), Some(b)) = (apex_a, apex_b) {
            faces_to_remove.insert(face_a as usize);
            faces_to_remove.insert(face_b as usize);

            // Create new quad (2 triangles) spanning the 4 vertices
            // from -> a -> b, from -> b -> to
            new_tris.push([from, a, b]);
            new_tris.push([from, b, to]);
        }
    }

    // Remove dissolved faces and add replacements
    let mut result_tris: Vec<[u32; 3]> = edit
        .triangles
        .iter()
        .enumerate()
        .filter(|(fi, _)| !faces_to_remove.contains(fi))
        .map(|(_, tri)| *tri)
        .collect();
    result_tris.extend(new_tris);

    edit.triangles = result_tris;
    edit.recompute_normals();

    HalfEdgeMesh::from_edit_mesh(&edit)
}

/// Dissolve selected vertices: remove each vertex and retriangulate the hole.
///
/// For each selected vertex, all adjacent faces are removed and the resulting
/// polygon hole is filled with a triangle fan from one of the remaining vertices.
pub fn dissolve_vertices(
    mesh: &HalfEdgeMesh,
    selected_verts: &HashSet<VertexId>,
) -> HalfEdgeMesh {
    if selected_verts.is_empty() {
        return mesh.clone();
    }

    let mut edit = mesh.to_edit_mesh();

    for &vert in selected_verts {
        // Find all faces using this vertex
        let faces_to_remove: HashSet<usize> = edit
            .triangles
            .iter()
            .enumerate()
            .filter(|(_, tri)| tri.contains(&vert))
            .map(|(fi, _)| fi)
            .collect();

        if faces_to_remove.is_empty() {
            continue;
        }

        // Collect the ring of vertices around the dissolved vertex (ordered)
        let mut ring_verts: Vec<u32> = Vec::new();
        for &fi in &faces_to_remove {
            for &v in &edit.triangles[fi] {
                if v != vert && !ring_verts.contains(&v) {
                    ring_verts.push(v);
                }
            }
        }

        // Remove old faces
        let mut remaining: Vec<[u32; 3]> = edit
            .triangles
            .iter()
            .enumerate()
            .filter(|(fi, _)| !faces_to_remove.contains(fi))
            .map(|(_, tri)| *tri)
            .collect();

        // Triangulate the hole with a fan from the first ring vertex
        if ring_verts.len() >= 3 {
            let hub = ring_verts[0];
            for i in 1..ring_verts.len() - 1 {
                remaining.push([hub, ring_verts[i], ring_verts[i + 1]]);
            }
        }

        edit.triangles = remaining;
    }

    edit.recompute_normals();
    HalfEdgeMesh::from_edit_mesh(&edit)
}
