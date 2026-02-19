//! Uniform triangle remeshing via iterative edge operations.
//!
//! Targets a uniform edge length by splitting long edges, collapsing short
//! edges, flipping edges to improve triangle quality, and smoothing.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::edit_mesh::EditMesh;

/// Remesh to target a uniform edge length.
///
/// Performs several iterations of: split long edges, collapse short edges,
/// flip edges to improve valence, then tangential smoothing.
pub fn remesh(mesh: &EditMesh, target_edge_length: f32) -> EditMesh {
    let mut state = RemeshState::from_edit_mesh(mesh);

    let high = target_edge_length * 4.0 / 3.0;
    let low = target_edge_length * 4.0 / 5.0;

    for _ in 0..5 {
        state.split_long_edges(high);
        state.collapse_short_edges(low);
        state.flip_edges_to_improve_valence();
        state.tangential_smooth(0.5);
    }

    state.to_edit_mesh()
}

/// Working state for remeshing — adjacency-rich triangle soup.
struct RemeshState {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    triangles: Vec<[u32; 3]>,
    alive: Vec<bool>,
}

impl RemeshState {
    fn from_edit_mesh(mesh: &EditMesh) -> Self {
        RemeshState {
            positions: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            uvs: mesh.uvs.clone(),
            triangles: mesh.triangles.clone(),
            alive: vec![true; mesh.triangles.len()],
        }
    }

    fn edge_length(&self, a: u32, b: u32) -> f32 {
        self.positions[a as usize].distance(self.positions[b as usize])
    }

    /// Split edges longer than `threshold` at their midpoint.
    fn split_long_edges(&mut self, threshold: f32) {
        let mut edges_to_split: Vec<(u32, u32)> = Vec::new();
        let mut seen = HashSet::new();

        for (ti, tri) in self.triangles.iter().enumerate() {
            if !self.alive[ti] {
                continue;
            }
            for i in 0..3 {
                let a = tri[i];
                let b = tri[(i + 1) % 3];
                let key = if a <= b { (a, b) } else { (b, a) };
                if seen.insert(key) && self.edge_length(a, b) > threshold {
                    edges_to_split.push(key);
                }
            }
        }

        for (a, b) in edges_to_split {
            self.split_edge(a, b);
        }
    }

    /// Split an edge by inserting a midpoint vertex and updating adjacent triangles.
    fn split_edge(&mut self, a: u32, b: u32) {
        let mid_pos = (self.positions[a as usize] + self.positions[b as usize]) * 0.5;
        let mid_nor = (self.normals[a as usize] + self.normals[b as usize]).normalize_or_zero();
        let mid_uv = (self.uvs[a as usize] + self.uvs[b as usize]) * 0.5;

        let mid = self.positions.len() as u32;
        self.positions.push(mid_pos);
        self.normals.push(mid_nor);
        self.uvs.push(mid_uv);

        // Find and split all triangles containing edge (a, b)
        let n = self.triangles.len();
        for ti in 0..n {
            if !self.alive[ti] {
                continue;
            }
            let tri = self.triangles[ti];
            let (has_a, has_b) = (
                tri.contains(&a),
                tri.contains(&b),
            );
            if !has_a || !has_b {
                continue;
            }

            // Find the opposite vertex
            let opp = tri.iter().copied().find(|&v| v != a && v != b).unwrap();

            // Kill old triangle, create two new ones
            self.alive[ti] = false;
            let t1 = [a, mid, opp];
            let t2 = [mid, b, opp];
            self.triangles.push(t1);
            self.alive.push(true);
            self.triangles.push(t2);
            self.alive.push(true);
        }
    }

    /// Collapse edges shorter than `threshold`.
    fn collapse_short_edges(&mut self, threshold: f32) {
        let mut edges: Vec<(u32, u32, f32)> = Vec::new();
        let mut seen = HashSet::new();

        for (ti, tri) in self.triangles.iter().enumerate() {
            if !self.alive[ti] {
                continue;
            }
            for i in 0..3 {
                let a = tri[i];
                let b = tri[(i + 1) % 3];
                let key = if a <= b { (a, b) } else { (b, a) };
                if seen.insert(key) {
                    let len = self.edge_length(a, b);
                    if len < threshold {
                        edges.push((key.0, key.1, len));
                    }
                }
            }
        }

        // Sort by length (shortest first)
        edges.sort_by(|a, b| a.2.partial_cmp(&b.2).unwrap_or(std::cmp::Ordering::Equal));

        let mut removed = HashSet::new();
        for (a, b, _) in edges {
            if removed.contains(&a) || removed.contains(&b) {
                continue;
            }
            // Collapse b into a (move a to midpoint)
            self.positions[a as usize] =
                (self.positions[a as usize] + self.positions[b as usize]) * 0.5;
            self.normals[a as usize] = (self.normals[a as usize] + self.normals[b as usize])
                .normalize_or_zero();
            self.uvs[a as usize] = (self.uvs[a as usize] + self.uvs[b as usize]) * 0.5;

            // Remap b → a in all triangles
            for (ti, tri) in self.triangles.iter_mut().enumerate() {
                if !self.alive[ti] {
                    continue;
                }
                for v in tri.iter_mut() {
                    if *v == b {
                        *v = a;
                    }
                }
                // Kill degenerate triangles
                if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                    self.alive[ti] = false;
                }
            }

            removed.insert(b);
        }
    }

    /// Flip edges to improve vertex valence (closer to 6 for interior vertices).
    fn flip_edges_to_improve_valence(&mut self) {
        let valence = self.compute_valence();

        // Build edge → two adjacent triangle indices
        let mut edge_tris: HashMap<(u32, u32), Vec<usize>> = HashMap::new();
        for (ti, tri) in self.triangles.iter().enumerate() {
            if !self.alive[ti] {
                continue;
            }
            for i in 0..3 {
                let a = tri[i];
                let b = tri[(i + 1) % 3];
                let key = if a <= b { (a, b) } else { (b, a) };
                edge_tris.entry(key).or_default().push(ti);
            }
        }

        for ((a, b), tris) in &edge_tris {
            if tris.len() != 2 {
                continue;
            }
            let t0 = tris[0];
            let t1 = tris[1];
            if !self.alive[t0] || !self.alive[t1] {
                continue;
            }

            let tri0 = self.triangles[t0];
            let tri1 = self.triangles[t1];

            let opp0 = tri0.iter().copied().find(|&v| v != *a && v != *b).unwrap();
            let opp1 = tri1.iter().copied().find(|&v| v != *a && v != *b).unwrap();

            if opp0 == opp1 {
                continue;
            }

            // Compute valence deviation before and after flip
            let target = 6i32;
            let before = [*a, *b, opp0, opp1]
                .iter()
                .map(|&v| {
                    let dev = valence.get(&v).copied().unwrap_or(6) as i32 - target;
                    dev * dev
                })
                .sum::<i32>();

            // After flip: a and b lose one connection, opp0 and opp1 gain one
            let va = valence.get(a).copied().unwrap_or(6) as i32 - 1;
            let vb = valence.get(b).copied().unwrap_or(6) as i32 - 1;
            let vo0 = valence.get(&opp0).copied().unwrap_or(6) as i32 + 1;
            let vo1 = valence.get(&opp1).copied().unwrap_or(6) as i32 + 1;
            let after = (va - target).pow(2)
                + (vb - target).pow(2)
                + (vo0 - target).pow(2)
                + (vo1 - target).pow(2);

            if after < before {
                // Flip: replace (a,b) with (opp0, opp1)
                self.triangles[t0] = [*a, opp1, opp0];
                self.triangles[t1] = [*b, opp0, opp1];
            }
        }
    }

    fn compute_valence(&self) -> HashMap<u32, u32> {
        let mut valence: HashMap<u32, u32> = HashMap::new();
        let mut seen = HashSet::new();
        for (ti, tri) in self.triangles.iter().enumerate() {
            if !self.alive[ti] {
                continue;
            }
            for i in 0..3 {
                let a = tri[i];
                let b = tri[(i + 1) % 3];
                let key = if a <= b { (a, b) } else { (b, a) };
                if seen.insert(key) {
                    *valence.entry(a).or_default() += 1;
                    *valence.entry(b).or_default() += 1;
                }
            }
        }
        valence
    }

    /// Tangential Laplacian smoothing: move vertices toward neighbor average,
    /// projected back onto the tangent plane to preserve shape.
    fn tangential_smooth(&mut self, factor: f32) {
        let mut neighbors: HashMap<u32, HashSet<u32>> = HashMap::new();
        for (ti, tri) in self.triangles.iter().enumerate() {
            if !self.alive[ti] {
                continue;
            }
            for i in 0..3 {
                let a = tri[i];
                let b = tri[(i + 1) % 3];
                neighbors.entry(a).or_default().insert(b);
                neighbors.entry(b).or_default().insert(a);
            }
        }

        let old = self.positions.clone();
        for (vi, pos) in self.positions.iter_mut().enumerate() {
            let vi = vi as u32;
            let Some(nbrs) = neighbors.get(&vi) else {
                continue;
            };
            if nbrs.is_empty() {
                continue;
            }

            let avg: Vec3 = nbrs.iter().map(|&n| old[n as usize]).sum::<Vec3>() / nbrs.len() as f32;
            let delta = avg - *pos;
            let normal = self.normals[vi as usize];

            // Project delta onto tangent plane
            let tangential = delta - normal * delta.dot(normal);
            *pos += tangential * factor;
        }
    }

    fn to_edit_mesh(&self) -> EditMesh {
        // Compact: collect live vertices and remap
        let mut used_verts = HashSet::new();
        for (ti, tri) in self.triangles.iter().enumerate() {
            if self.alive[ti] {
                for &v in tri {
                    used_verts.insert(v);
                }
            }
        }

        let mut old_to_new: HashMap<u32, u32> = HashMap::new();
        let mut positions = Vec::new();
        let mut normals = Vec::new();
        let mut uvs = Vec::new();

        let mut sorted: Vec<u32> = used_verts.into_iter().collect();
        sorted.sort();
        for v in sorted {
            old_to_new.insert(v, positions.len() as u32);
            positions.push(self.positions[v as usize]);
            normals.push(self.normals[v as usize]);
            uvs.push(self.uvs[v as usize]);
        }

        let mut triangles = Vec::new();
        for (ti, tri) in self.triangles.iter().enumerate() {
            if !self.alive[ti] {
                continue;
            }
            if let (Some(&a), Some(&b), Some(&c)) = (
                old_to_new.get(&tri[0]),
                old_to_new.get(&tri[1]),
                old_to_new.get(&tri[2]),
            ) {
                triangles.push([a, b, c]);
            }
        }

        let mut mesh = EditMesh {
            positions,
            normals,
            uvs,
            triangles,
        };
        mesh.recompute_normals();
        mesh
    }
}
