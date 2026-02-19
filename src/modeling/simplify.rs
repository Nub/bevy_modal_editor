//! Mesh simplification using Quadric Error Metrics (QEM).
//!
//! Iteratively collapses the edge with the lowest error cost until the
//! target triangle count is reached.

use bevy::prelude::*;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::cmp::Ordering;

use super::edit_mesh::EditMesh;

/// Simplify a mesh by collapsing edges until the triangle count is reduced
/// to `target_ratio` of the original (0.0–1.0).
///
/// Uses Quadric Error Metrics to choose which edges to collapse with
/// minimal visual impact.
pub fn simplify_mesh(mesh: &EditMesh, target_ratio: f32) -> EditMesh {
    let target_ratio = target_ratio.clamp(0.01, 1.0);
    let target_tris = ((mesh.triangles.len() as f32 * target_ratio).ceil() as usize).max(1);

    if mesh.triangles.len() <= target_tris {
        return mesh.clone();
    }

    let mut state = SimplifyState::new(mesh);

    while state.live_tri_count > target_tris {
        if !state.collapse_cheapest() {
            break; // No more collapsible edges
        }
    }

    state.build_mesh()
}

/// 4x4 symmetric matrix for quadric error computation.
#[derive(Debug, Clone, Copy)]
struct Quadric {
    data: [f64; 10], // Symmetric 4x4 stored as upper triangle
}

impl Quadric {
    fn zero() -> Self {
        Quadric { data: [0.0; 10] }
    }

    /// Create a quadric from a plane equation (nx, ny, nz, d) where nx*x + ny*y + nz*z + d = 0.
    fn from_plane(a: f64, b: f64, c: f64, d: f64) -> Self {
        Quadric {
            data: [
                a * a, a * b, a * c, a * d,
                       b * b, b * c, b * d,
                              c * c, c * d,
                                     d * d,
            ],
        }
    }

    fn add(&self, other: &Quadric) -> Quadric {
        let mut result = Quadric::zero();
        for i in 0..10 {
            result.data[i] = self.data[i] + other.data[i];
        }
        result
    }

    /// Evaluate the quadric error for a point.
    fn evaluate(&self, v: Vec3) -> f64 {
        let x = v.x as f64;
        let y = v.y as f64;
        let z = v.z as f64;
        let d = &self.data;
        d[0] * x * x + 2.0 * d[1] * x * y + 2.0 * d[2] * x * z + 2.0 * d[3] * x
            + d[4] * y * y + 2.0 * d[5] * y * z + 2.0 * d[6] * y
            + d[7] * z * z + 2.0 * d[8] * z
            + d[9]
    }
}

/// Edge collapse candidate in the priority queue.
struct CollapseCandidate {
    cost: f64,
    v0: u32,
    v1: u32,
    target_pos: Vec3,
}

impl PartialEq for CollapseCandidate {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}

impl Eq for CollapseCandidate {}

impl PartialOrd for CollapseCandidate {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for CollapseCandidate {
    fn cmp(&self, other: &Self) -> Ordering {
        // Reverse ordering for min-heap (BinaryHeap is a max-heap)
        other.cost.partial_cmp(&self.cost).unwrap_or(Ordering::Equal)
    }
}

struct SimplifyState {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    triangles: Vec<[u32; 3]>,
    tri_alive: Vec<bool>,
    vert_alive: Vec<bool>,
    quadrics: Vec<Quadric>,
    heap: BinaryHeap<CollapseCandidate>,
    // Maps each vertex to its collapse target (union-find style)
    remap: Vec<u32>,
    live_tri_count: usize,
}

impl SimplifyState {
    fn new(mesh: &EditMesh) -> Self {
        let n_verts = mesh.positions.len();
        let n_tris = mesh.triangles.len();

        // Initialize per-vertex quadrics from face planes
        let mut quadrics = vec![Quadric::zero(); n_verts];
        for tri in &mesh.triangles {
            let [a, b, c] = *tri;
            let p0 = mesh.positions[a as usize];
            let p1 = mesh.positions[b as usize];
            let p2 = mesh.positions[c as usize];
            let normal = (p1 - p0).cross(p2 - p0).normalize_or_zero();
            let d = -(normal.dot(p0) as f64);
            let q = Quadric::from_plane(normal.x as f64, normal.y as f64, normal.z as f64, d);
            quadrics[a as usize] = quadrics[a as usize].add(&q);
            quadrics[b as usize] = quadrics[b as usize].add(&q);
            quadrics[c as usize] = quadrics[c as usize].add(&q);
        }

        let remap: Vec<u32> = (0..n_verts as u32).collect();

        let mut state = SimplifyState {
            positions: mesh.positions.clone(),
            normals: mesh.normals.clone(),
            uvs: mesh.uvs.clone(),
            triangles: mesh.triangles.clone(),
            tri_alive: vec![true; n_tris],
            vert_alive: vec![true; n_verts],
            quadrics,
            heap: BinaryHeap::new(),
            remap,
            live_tri_count: n_tris,
        };

        // Build initial edge collapse candidates
        let mut edges_seen = HashSet::new();
        for tri in &mesh.triangles {
            for i in 0..3 {
                let a = tri[i];
                let b = tri[(i + 1) % 3];
                let edge = if a <= b { (a, b) } else { (b, a) };
                if edges_seen.insert(edge) {
                    state.push_edge(edge.0, edge.1);
                }
            }
        }

        state
    }

    fn resolve(&self, mut v: u32) -> u32 {
        while self.remap[v as usize] != v {
            v = self.remap[v as usize];
        }
        v
    }

    fn push_edge(&mut self, v0: u32, v1: u32) {
        let q = self.quadrics[v0 as usize].add(&self.quadrics[v1 as usize]);
        // Try midpoint as target position
        let mid = (self.positions[v0 as usize] + self.positions[v1 as usize]) * 0.5;
        let cost = q.evaluate(mid);

        self.heap.push(CollapseCandidate {
            cost,
            v0,
            v1,
            target_pos: mid,
        });
    }

    fn collapse_cheapest(&mut self) -> bool {
        while let Some(candidate) = self.heap.pop() {
            let v0 = self.resolve(candidate.v0);
            let v1 = self.resolve(candidate.v1);

            // Skip stale entries
            if v0 == v1 || !self.vert_alive[v0 as usize] || !self.vert_alive[v1 as usize] {
                continue;
            }

            // Collapse v1 into v0
            self.positions[v0 as usize] = candidate.target_pos;
            self.normals[v0 as usize] = (self.normals[v0 as usize]
                + self.normals[v1 as usize])
                .normalize_or_zero();
            self.uvs[v0 as usize] =
                (self.uvs[v0 as usize] + self.uvs[v1 as usize]) * 0.5;
            self.quadrics[v0 as usize] = self.quadrics[v0 as usize].add(&self.quadrics[v1 as usize]);

            self.vert_alive[v1 as usize] = false;
            self.remap[v1 as usize] = v0;

            // Pre-resolve all remap targets so we don't borrow self during mutation
            let mut resolved: Vec<u32> = (0..self.remap.len() as u32).collect();
            for i in 0..resolved.len() {
                resolved[i] = self.resolve(i as u32);
            }

            // Update triangles and kill degenerate ones
            let mut neighbor_verts = HashSet::new();
            for (ti, tri) in self.triangles.iter_mut().enumerate() {
                if !self.tri_alive[ti] {
                    continue;
                }
                // Remap all vertices using pre-resolved table
                for v in tri.iter_mut() {
                    *v = resolved[*v as usize];
                }

                // Kill degenerate triangles (two or more identical vertices)
                if tri[0] == tri[1] || tri[1] == tri[2] || tri[0] == tri[2] {
                    self.tri_alive[ti] = false;
                    self.live_tri_count -= 1;
                } else {
                    // Collect neighbors for re-evaluation
                    for &v in tri.iter() {
                        if v != v0 {
                            neighbor_verts.insert(v);
                        }
                    }
                }
            }

            // Re-push edges from v0 to its neighbors
            for &nv in &neighbor_verts {
                if self.vert_alive[nv as usize] {
                    self.push_edge(v0, nv);
                }
            }

            return true;
        }

        false
    }

    fn build_mesh(&self) -> EditMesh {
        // Collect live vertices and remap indices
        let mut new_positions = Vec::new();
        let mut new_normals = Vec::new();
        let mut new_uvs = Vec::new();
        let mut old_to_new: HashMap<u32, u32> = HashMap::new();

        for (i, alive) in self.vert_alive.iter().enumerate() {
            if *alive {
                let idx = new_positions.len() as u32;
                old_to_new.insert(i as u32, idx);
                new_positions.push(self.positions[i]);
                new_normals.push(self.normals[i]);
                new_uvs.push(self.uvs[i]);
            }
        }

        let mut new_triangles = Vec::new();
        for (ti, tri) in self.triangles.iter().enumerate() {
            if !self.tri_alive[ti] {
                continue;
            }
            let a = self.resolve(tri[0]);
            let b = self.resolve(tri[1]);
            let c = self.resolve(tri[2]);
            if a == b || b == c || a == c {
                continue;
            }
            if let (Some(&na), Some(&nb), Some(&nc)) =
                (old_to_new.get(&a), old_to_new.get(&b), old_to_new.get(&c))
            {
                new_triangles.push([na, nb, nc]);
            }
        }

        let mut mesh = EditMesh {
            positions: new_positions,
            normals: new_normals,
            uvs: new_uvs,
            triangles: new_triangles,
        };
        mesh.recompute_normals();
        mesh
    }
}
