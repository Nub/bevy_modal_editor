//! Vertex welding (merge) operation for the mesh modeling tool.
//!
//! Merges selected vertices that are within a distance threshold, relinking
//! all triangle references to use the surviving vertex.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::edit_mesh::EditMesh;

/// Weld (merge) selected vertices that are within `threshold` distance of each other.
///
/// For each cluster of nearby vertices, one is kept and all others are remapped.
/// Degenerate triangles (where two or more vertices collapse to the same index)
/// are removed.
///
/// Returns the modified mesh.
pub fn weld_vertices(
    mesh: &EditMesh,
    selected: &HashSet<u32>,
    threshold: f32,
) -> EditMesh {
    if selected.len() < 2 || threshold <= 0.0 {
        return mesh.clone();
    }

    let threshold_sq = threshold * threshold;

    // Build clusters: union-find style grouping of nearby vertices
    let selected_vec: Vec<u32> = selected.iter().copied().collect();
    let mut parent: HashMap<u32, u32> = HashMap::new();
    for &v in &selected_vec {
        parent.insert(v, v);
    }

    // Find root of a vertex in the union-find
    fn find(parent: &mut HashMap<u32, u32>, v: u32) -> u32 {
        let p = parent[&v];
        if p == v {
            return v;
        }
        let root = find(parent, p);
        parent.insert(v, root);
        root
    }

    // Union two vertices
    fn union(parent: &mut HashMap<u32, u32>, a: u32, b: u32) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            // Keep the lower index as root
            if ra < rb {
                parent.insert(rb, ra);
            } else {
                parent.insert(ra, rb);
            }
        }
    }

    // Cluster nearby vertices
    for i in 0..selected_vec.len() {
        for j in (i + 1)..selected_vec.len() {
            let vi = selected_vec[i];
            let vj = selected_vec[j];
            if (vi as usize) < mesh.positions.len() && (vj as usize) < mesh.positions.len() {
                let dist_sq = mesh.positions[vi as usize]
                    .distance_squared(mesh.positions[vj as usize]);
                if dist_sq <= threshold_sq {
                    union(&mut parent, vi, vj);
                }
            }
        }
    }

    // Build remap: each vertex -> its cluster root
    let mut remap: HashMap<u32, u32> = HashMap::new();
    for &v in &selected_vec {
        let root = find(&mut parent, v);
        if root != v {
            remap.insert(v, root);
        }
    }

    if remap.is_empty() {
        return mesh.clone();
    }

    // Average positions for each cluster root
    let mut cluster_sums: HashMap<u32, (Vec3, Vec3, Vec2, usize)> = HashMap::new();
    for &v in &selected_vec {
        let root = find(&mut parent, v);
        if (v as usize) < mesh.positions.len() {
            let entry = cluster_sums.entry(root).or_insert((Vec3::ZERO, Vec3::ZERO, Vec2::ZERO, 0));
            entry.0 += mesh.positions[v as usize];
            entry.1 += mesh.normals[v as usize];
            entry.2 += mesh.uvs[v as usize];
            entry.3 += 1;
        }
    }

    let mut new_positions = mesh.positions.clone();
    let mut new_normals = mesh.normals.clone();
    let mut new_uvs = mesh.uvs.clone();

    for (&root, &(pos_sum, nor_sum, uv_sum, count)) in &cluster_sums {
        if (root as usize) < new_positions.len() && count > 0 {
            new_positions[root as usize] = pos_sum / count as f32;
            new_normals[root as usize] = (nor_sum / count as f32).normalize_or_zero();
            new_uvs[root as usize] = uv_sum / count as f32;
        }
    }

    // Remap triangles and remove degenerates
    let mut new_triangles = Vec::with_capacity(mesh.triangles.len());
    for tri in &mesh.triangles {
        let mut new_tri = *tri;
        for v in new_tri.iter_mut() {
            if let Some(&target) = remap.get(v) {
                *v = target;
            }
        }
        // Skip degenerate triangles
        if new_tri[0] != new_tri[1] && new_tri[1] != new_tri[2] && new_tri[2] != new_tri[0] {
            new_triangles.push(new_tri);
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
