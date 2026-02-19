//! Normal editing: auto-smooth by angle, hard/soft edge marking, recalculate.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::edit_mesh::{Edge, EditMesh, FaceIndex};

/// Recompute normals with auto-smooth: faces with a shared edge whose normals
/// differ by more than `angle_threshold_degrees` get hard (split) normals,
/// otherwise they share smooth normals.
pub fn auto_smooth_normals(mesh: &EditMesh, angle_threshold_degrees: f32) -> EditMesh {
    auto_smooth_normals_with_hard_edges(mesh, angle_threshold_degrees, &HashSet::new())
}

/// Recompute normals with auto-smooth, also treating `hard_edges` as forced
/// hard edges regardless of angle.
pub fn auto_smooth_normals_with_hard_edges(
    mesh: &EditMesh,
    angle_threshold_degrees: f32,
    hard_edges: &HashSet<Edge>,
) -> EditMesh {
    let threshold_cos = angle_threshold_degrees.to_radians().cos();
    let adj = mesh.build_adjacency();

    // Step 1: Determine which edges are "hard" (need split normals)
    let mut all_hard_edges = hard_edges.clone();
    for (edge, faces) in &adj {
        if faces.len() == 2 {
            let n0 = mesh.face_normal(faces[0]);
            let n1 = mesh.face_normal(faces[1]);
            if n0.dot(n1) < threshold_cos {
                all_hard_edges.insert(*edge);
            }
        }
    }

    // Step 2: Build smooth groups — connected components of faces separated by hard edges
    let mut visited = HashSet::new();
    let mut smooth_groups: Vec<HashSet<FaceIndex>> = Vec::new();

    for fi in 0..mesh.triangles.len() {
        if visited.contains(&fi) {
            continue;
        }

        let mut group = HashSet::new();
        let mut frontier = vec![fi];
        while let Some(face) = frontier.pop() {
            if !group.insert(face) {
                continue;
            }
            visited.insert(face);

            for edge in mesh.face_edges(face) {
                if all_hard_edges.contains(&edge) {
                    continue; // Don't cross hard edges
                }
                if let Some(neighbors) = adj.get(&edge) {
                    for &n in neighbors {
                        if !group.contains(&n) {
                            frontier.push(n);
                        }
                    }
                }
            }
        }
        smooth_groups.push(group);
    }

    // Step 3: For each smooth group, compute shared vertex normals
    // We need to split vertices shared across different smooth groups
    let mut result_positions = Vec::new();
    let mut result_normals = Vec::new();
    let mut result_uvs = Vec::new();
    let mut result_triangles = Vec::new();

    // Map: (original_vertex, smooth_group_index) -> new_vertex_index
    let mut vertex_map: HashMap<(u32, usize), u32> = HashMap::new();

    for (group_idx, group) in smooth_groups.iter().enumerate() {
        for &fi in group {
            let old_tri = mesh.triangles[fi];
            let mut new_tri = [0u32; 3];

            for (i, &old_vi) in old_tri.iter().enumerate() {
                let key = (old_vi, group_idx);
                let new_vi = *vertex_map.entry(key).or_insert_with(|| {
                    let idx = result_positions.len() as u32;
                    result_positions.push(mesh.positions[old_vi as usize]);
                    result_normals.push(Vec3::ZERO); // Will compute below
                    result_uvs.push(mesh.uvs[old_vi as usize]);
                    idx
                });
                new_tri[i] = new_vi;
            }

            result_triangles.push(new_tri);
        }
    }

    // Step 4: Compute smooth normals per new vertex
    for tri in &result_triangles {
        let v0 = result_positions[tri[0] as usize];
        let v1 = result_positions[tri[1] as usize];
        let v2 = result_positions[tri[2] as usize];
        let face_normal = (v1 - v0).cross(v2 - v0);
        for &vi in tri {
            result_normals[vi as usize] += face_normal;
        }
    }

    for n in &mut result_normals {
        *n = n.normalize_or_zero();
    }

    EditMesh {
        positions: result_positions,
        normals: result_normals,
        uvs: result_uvs,
        triangles: result_triangles,
    }
}

/// Recalculate flat normals (each face gets its own normal, vertices are split).
pub fn flat_normals(mesh: &EditMesh) -> EditMesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut triangles = Vec::new();

    for (fi, tri) in mesh.triangles.iter().enumerate() {
        let base = positions.len() as u32;
        let n = mesh.face_normal(fi);

        for &vi in tri {
            positions.push(mesh.positions[vi as usize]);
            normals.push(n);
            uvs.push(mesh.uvs[vi as usize]);
        }

        triangles.push([base, base + 1, base + 2]);
    }

    EditMesh {
        positions,
        normals,
        uvs,
        triangles,
    }
}

/// Toggle an edge as hard/soft in the given set. Returns true if added (now hard).
pub fn toggle_hard_edge(hard_edges: &mut HashSet<Edge>, a: u32, b: u32) -> bool {
    let edge = Edge::new(a, b);
    if hard_edges.contains(&edge) {
        hard_edges.remove(&edge);
        false
    } else {
        hard_edges.insert(edge);
        true
    }
}
