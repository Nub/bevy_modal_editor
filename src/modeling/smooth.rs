//! Mesh smoothing and subdivision.
//!
//! Provides Laplacian smoothing (iterative vertex averaging) and
//! midpoint subdivision (split each triangle into 4).

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::edit_mesh::EditMesh;

/// Laplacian smoothing: iteratively move each vertex toward the average
/// of its neighbors.
///
/// `factor` controls blend strength (0 = no change, 1 = full average).
/// Boundary vertices (edges with only one adjacent face) are not moved
/// to preserve the mesh silhouette.
pub fn smooth_mesh(mesh: &EditMesh, iterations: u32, factor: f32) -> EditMesh {
    let mut result = mesh.clone();
    let factor = factor.clamp(0.0, 1.0);

    // Build vertex adjacency: for each vertex, the set of connected vertices
    let mut neighbors: Vec<HashSet<u32>> = vec![HashSet::new(); result.positions.len()];
    for tri in &result.triangles {
        neighbors[tri[0] as usize].insert(tri[1]);
        neighbors[tri[0] as usize].insert(tri[2]);
        neighbors[tri[1] as usize].insert(tri[0]);
        neighbors[tri[1] as usize].insert(tri[2]);
        neighbors[tri[2] as usize].insert(tri[0]);
        neighbors[tri[2] as usize].insert(tri[1]);
    }

    // Find boundary vertices (on edges with only one face)
    let boundary = find_boundary_vertices(mesh);

    for _ in 0..iterations {
        let old_positions = result.positions.clone();
        for (vi, pos) in result.positions.iter_mut().enumerate() {
            if boundary.contains(&(vi as u32)) {
                continue; // Don't move boundary vertices
            }
            let nbrs = &neighbors[vi];
            if nbrs.is_empty() {
                continue;
            }
            let avg: Vec3 = nbrs
                .iter()
                .map(|&ni| old_positions[ni as usize])
                .sum::<Vec3>()
                / nbrs.len() as f32;
            *pos = pos.lerp(avg, factor);
        }
    }

    result.recompute_normals();
    result
}

/// Midpoint subdivision: split each triangle into 4 by inserting vertices
/// at edge midpoints.
///
/// Each original triangle ABC becomes 4 triangles:
///   A-AB-CA, AB-B-BC, CA-BC-C, AB-BC-CA
/// where AB, BC, CA are the edge midpoints.
pub fn subdivide_mesh(mesh: &EditMesh) -> EditMesh {
    let mut positions = mesh.positions.clone();
    let mut normals = mesh.normals.clone();
    let mut uvs = mesh.uvs.clone();
    let mut triangles = Vec::new();

    // Cache: edge → midpoint vertex index (to avoid duplicates)
    let mut edge_midpoints: HashMap<(u32, u32), u32> = HashMap::new();

    let get_midpoint = |a: u32, b: u32,
                            positions: &mut Vec<Vec3>,
                            normals: &mut Vec<Vec3>,
                            uvs: &mut Vec<Vec2>,
                            cache: &mut HashMap<(u32, u32), u32>|
     -> u32 {
        let key = if a <= b { (a, b) } else { (b, a) };
        if let Some(&idx) = cache.get(&key) {
            return idx;
        }
        let idx = positions.len() as u32;
        positions.push((positions[a as usize] + positions[b as usize]) * 0.5);
        normals.push(
            (normals[a as usize] + normals[b as usize])
                .normalize_or_zero(),
        );
        uvs.push((uvs[a as usize] + uvs[b as usize]) * 0.5);
        cache.insert(key, idx);
        idx
    };

    for tri in &mesh.triangles {
        let [a, b, c] = *tri;
        let ab = get_midpoint(a, b, &mut positions, &mut normals, &mut uvs, &mut edge_midpoints);
        let bc = get_midpoint(b, c, &mut positions, &mut normals, &mut uvs, &mut edge_midpoints);
        let ca = get_midpoint(c, a, &mut positions, &mut normals, &mut uvs, &mut edge_midpoints);

        triangles.push([a, ab, ca]);
        triangles.push([ab, b, bc]);
        triangles.push([ca, bc, c]);
        triangles.push([ab, bc, ca]);
    }

    let mut result = EditMesh {
        positions,
        normals,
        uvs,
        triangles,
    };
    result.recompute_normals();
    result
}

/// Find vertices that lie on boundary edges (edges with only one adjacent face).
fn find_boundary_vertices(mesh: &EditMesh) -> HashSet<u32> {
    let adj = mesh.build_adjacency();
    let mut boundary = HashSet::new();
    for (edge, faces) in &adj {
        if faces.len() == 1 {
            boundary.insert(edge.0);
            boundary.insert(edge.1);
        }
    }
    boundary
}
