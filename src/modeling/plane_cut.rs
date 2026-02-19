//! Plane-based mesh slicing.
//!
//! Slices a mesh with an arbitrary plane, producing two separate meshes
//! (one on each side of the plane) with new triangles along the cut.

use bevy::prelude::*;
use std::collections::HashMap;

use super::edit_mesh::EditMesh;

/// Slice a mesh with a plane defined by a point and normal.
///
/// Returns (front, back) where front is the half on the positive side
/// of the plane normal, and back is the other half.
/// New vertices are created along the cut boundary.
pub fn plane_cut(
    mesh: &EditMesh,
    plane_point: Vec3,
    plane_normal: Vec3,
) -> (EditMesh, EditMesh) {
    let normal = plane_normal.normalize_or_zero();

    // Classify each vertex as front (+), back (-), or on the plane
    let distances: Vec<f32> = mesh
        .positions
        .iter()
        .map(|p| (*p - plane_point).dot(normal))
        .collect();

    let mut front = MeshBuilder::new();
    let mut back = MeshBuilder::new();

    for tri in &mesh.triangles {
        let [a, b, c] = *tri;
        let da = distances[a as usize];
        let db = distances[b as usize];
        let dc = distances[c as usize];

        let sa = classify(da);
        let sb = classify(db);
        let sc = classify(dc);

        // All on one side
        if sa >= 0 && sb >= 0 && sc >= 0 {
            front.add_triangle(mesh, a, b, c);
            continue;
        }
        if sa <= 0 && sb <= 0 && sc <= 0 {
            back.add_triangle(mesh, a, b, c);
            continue;
        }

        // Triangle straddles the plane — split it
        split_triangle(
            mesh, &distances, a, b, c, &normal, &plane_point, &mut front, &mut back,
        );
    }

    (front.build(), back.build())
}

const EPSILON: f32 = 1e-6;

fn classify(distance: f32) -> i32 {
    if distance > EPSILON {
        1
    } else if distance < -EPSILON {
        -1
    } else {
        0
    }
}

/// Split a single triangle by the plane, adding resulting sub-triangles
/// to front and/or back builders.
fn split_triangle(
    mesh: &EditMesh,
    distances: &[f32],
    a: u32,
    b: u32,
    c: u32,
    _normal: &Vec3,
    _plane_point: &Vec3,
    front: &mut MeshBuilder,
    back: &mut MeshBuilder,
) {
    let verts = [a, b, c];
    let dists = [
        distances[a as usize],
        distances[b as usize],
        distances[c as usize],
    ];

    // Count vertices on each side
    let mut front_verts = Vec::new();
    let mut back_verts = Vec::new();

    for i in 0..3 {
        let j = (i + 1) % 3;
        let vi = verts[i];
        let vj = verts[j];
        let di = dists[i];
        let dj = dists[j];

        if di >= -EPSILON {
            front_verts.push(vi);
        }
        if di <= EPSILON {
            back_verts.push(vi);
        }

        // Check if edge crosses the plane
        if (di > EPSILON && dj < -EPSILON) || (di < -EPSILON && dj > EPSILON) {
            let t = di / (di - dj);
            let new_pos = mesh.positions[vi as usize].lerp(mesh.positions[vj as usize], t);
            let new_nor = mesh.normals[vi as usize]
                .lerp(mesh.normals[vj as usize], t)
                .normalize_or_zero();
            let new_uv = mesh.uvs[vi as usize].lerp(mesh.uvs[vj as usize], t);

            let new_front = front.add_vertex(new_pos, new_nor, new_uv);
            let new_back = back.add_vertex(new_pos, new_nor, new_uv);

            front_verts.push(new_front);
            back_verts.push(new_back);
        }
    }

    // Triangulate each side (fan from first vertex)
    triangulate_fan(&front_verts, mesh, front);
    triangulate_fan(&back_verts, mesh, back);
}

/// Fan-triangulate a polygon of vertex indices into a MeshBuilder.
fn triangulate_fan(verts: &[u32], _mesh: &EditMesh, builder: &mut MeshBuilder) {
    if verts.len() < 3 {
        return;
    }
    for i in 1..verts.len() - 1 {
        builder.add_raw_triangle(verts[0], verts[i], verts[i + 1]);
    }
}

/// Helper to accumulate vertices and triangles for one side of the cut.
struct MeshBuilder {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    triangles: Vec<[u32; 3]>,
    // Maps original vertex index → new vertex index in this builder
    vertex_map: HashMap<u32, u32>,
}

impl MeshBuilder {
    fn new() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            triangles: Vec::new(),
            vertex_map: HashMap::new(),
        }
    }

    /// Map an original vertex to this builder, adding it if not yet present.
    fn map_vertex(&mut self, mesh: &EditMesh, original: u32) -> u32 {
        if let Some(&idx) = self.vertex_map.get(&original) {
            return idx;
        }
        let idx = self.positions.len() as u32;
        self.positions.push(mesh.positions[original as usize]);
        self.normals.push(mesh.normals[original as usize]);
        self.uvs.push(mesh.uvs[original as usize]);
        self.vertex_map.insert(original, idx);
        idx
    }

    /// Add a brand new vertex (for intersection points). Returns the new index.
    fn add_vertex(&mut self, pos: Vec3, nor: Vec3, uv: Vec2) -> u32 {
        let idx = self.positions.len() as u32;
        self.positions.push(pos);
        self.normals.push(nor);
        self.uvs.push(uv);
        idx
    }

    /// Add a triangle using original vertex indices (maps them automatically).
    fn add_triangle(&mut self, mesh: &EditMesh, a: u32, b: u32, c: u32) {
        let ma = self.map_vertex(mesh, a);
        let mb = self.map_vertex(mesh, b);
        let mc = self.map_vertex(mesh, c);
        self.triangles.push([ma, mb, mc]);
    }

    /// Add a triangle using already-mapped vertex indices.
    fn add_raw_triangle(&mut self, a: u32, b: u32, c: u32) {
        self.triangles.push([a, b, c]);
    }

    fn build(self) -> EditMesh {
        let mut mesh = EditMesh {
            positions: self.positions,
            normals: self.normals,
            uvs: self.uvs,
            triangles: self.triangles,
        };
        mesh.recompute_normals();
        mesh
    }
}
