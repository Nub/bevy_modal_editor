//! Selection expansion tools: grow, shrink, select linked, select by normal.

use std::collections::HashSet;

use super::edit_mesh::{EditMesh, FaceIndex};
use super::half_edge::HalfEdgeMesh;

/// Grow face selection by one ring of adjacent faces.
pub fn grow_face_selection(mesh: &EditMesh, selected: &HashSet<FaceIndex>) -> HashSet<FaceIndex> {
    let adj = mesh.build_adjacency();
    let mut grown = selected.clone();

    for &fi in selected {
        if fi >= mesh.triangles.len() {
            continue;
        }
        for edge in mesh.face_edges(fi) {
            if let Some(neighbors) = adj.get(&edge) {
                for &neighbor in neighbors {
                    grown.insert(neighbor);
                }
            }
        }
    }

    grown
}

/// Shrink face selection by removing faces on the boundary of the selection.
pub fn shrink_face_selection(mesh: &EditMesh, selected: &HashSet<FaceIndex>) -> HashSet<FaceIndex> {
    let adj = mesh.build_adjacency();
    let mut boundary_faces = HashSet::new();

    for &fi in selected {
        if fi >= mesh.triangles.len() {
            continue;
        }
        let is_boundary = mesh.face_edges(fi).iter().any(|edge| {
            adj.get(edge)
                .map(|neighbors| neighbors.iter().any(|n| !selected.contains(n)))
                .unwrap_or(true) // edge with only one face is boundary
        });
        if is_boundary {
            boundary_faces.insert(fi);
        }
    }

    selected.difference(&boundary_faces).copied().collect()
}

/// Select all faces connected to the current selection (flood-fill ignoring normals).
pub fn select_linked_faces(mesh: &EditMesh, selected: &HashSet<FaceIndex>) -> HashSet<FaceIndex> {
    let adj = mesh.build_adjacency();
    let mut result = selected.clone();
    let mut frontier: Vec<FaceIndex> = selected.iter().copied().collect();

    while let Some(fi) = frontier.pop() {
        for edge in mesh.face_edges(fi) {
            if let Some(neighbors) = adj.get(&edge) {
                for &neighbor in neighbors {
                    if result.insert(neighbor) {
                        frontier.push(neighbor);
                    }
                }
            }
        }
    }

    result
}

/// Select all faces with normals similar to the currently selected faces.
///
/// `angle_threshold_degrees` controls the maximum angular deviation from the
/// average normal of the selection.
pub fn select_by_normal(
    mesh: &EditMesh,
    selected: &HashSet<FaceIndex>,
    angle_threshold_degrees: f32,
) -> HashSet<FaceIndex> {
    if selected.is_empty() {
        return HashSet::new();
    }

    // Compute area-weighted average normal of selection
    let mut avg_normal = bevy::prelude::Vec3::ZERO;
    for &fi in selected {
        if fi < mesh.triangles.len() {
            avg_normal += mesh.face_normal(fi) * mesh.face_area(fi);
        }
    }
    avg_normal = avg_normal.normalize_or_zero();

    if avg_normal == bevy::prelude::Vec3::ZERO {
        return selected.clone();
    }

    let threshold_cos = angle_threshold_degrees.to_radians().cos();
    let mut result = HashSet::new();

    for fi in 0..mesh.triangles.len() {
        if mesh.face_normal(fi).dot(avg_normal) >= threshold_cos {
            result.insert(fi);
        }
    }

    result
}

/// Grow vertex selection by one ring of adjacent vertices.
pub fn grow_vertex_selection(he_mesh: &HalfEdgeMesh, selected: &HashSet<u32>) -> HashSet<u32> {
    let mut grown = selected.clone();

    for &vi in selected {
        for neighbor in he_mesh.vertex_neighbors(vi) {
            grown.insert(neighbor);
        }
    }

    grown
}

/// Shrink vertex selection by removing vertices on the boundary.
pub fn shrink_vertex_selection(he_mesh: &HalfEdgeMesh, selected: &HashSet<u32>) -> HashSet<u32> {
    let mut result = selected.clone();

    for &vi in selected {
        let all_neighbors_selected = he_mesh
            .vertex_neighbors(vi)
            .iter()
            .all(|n| selected.contains(n));
        if !all_neighbors_selected {
            result.remove(&vi);
        }
    }

    result
}

/// Grow edge selection by one ring.
pub fn grow_edge_selection(he_mesh: &HalfEdgeMesh, selected: &HashSet<u32>) -> HashSet<u32> {
    let mut grown = selected.clone();

    for &he_id in selected {
        let (from, to) = he_mesh.edge_vertices(he_id);
        // Add all edges emanating from both endpoints
        for vi in [from, to] {
            for edge in he_mesh.vertex_edges(vi) {
                grown.insert(edge);
            }
        }
    }

    grown
}

/// Shrink edge selection by removing edges on the boundary.
pub fn shrink_edge_selection(he_mesh: &HalfEdgeMesh, selected: &HashSet<u32>) -> HashSet<u32> {
    let mut result = selected.clone();

    for &he_id in selected {
        let (from, to) = he_mesh.edge_vertices(he_id);
        // An edge is on the selection boundary if either endpoint has an
        // unselected edge emanating from it
        let is_boundary = [from, to].iter().any(|&vi| {
            he_mesh
                .vertex_edges(vi)
                .iter()
                .any(|&e| !selected.contains(&e))
        });
        if is_boundary {
            result.remove(&he_id);
        }
    }

    result
}
