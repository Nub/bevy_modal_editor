//! CSG boolean operations on meshes.
//!
//! Provides union, subtract, and intersect operations between two meshes
//! using a BSP-tree approach.

use bevy::prelude::*;
use std::collections::HashMap;

use super::edit_mesh::EditMesh;

/// Boolean operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BooleanOp {
    Union,
    Subtract,
    Intersect,
}

impl BooleanOp {
    pub fn display_name(&self) -> &'static str {
        match self {
            BooleanOp::Union => "Union",
            BooleanOp::Subtract => "Subtract",
            BooleanOp::Intersect => "Intersect",
        }
    }
}

/// Perform a CSG boolean operation on two meshes.
///
/// Both meshes should be in the same coordinate space (local space of the
/// primary entity). Transform mesh B into A's local space before calling.
///
/// Uses BSP-tree classification: each mesh's triangles are classified as
/// inside or outside the other, then kept or discarded based on the operation.
pub fn boolean_op(a: &EditMesh, b: &EditMesh, op: BooleanOp) -> EditMesh {
    let tree_a = BspTree::build(a);
    let tree_b = BspTree::build(b);

    let (a_inside, a_outside) = classify_triangles(a, &tree_b);
    let (b_inside, b_outside) = classify_triangles(b, &tree_a);

    let mut builder = MeshBuilder::new();

    match op {
        BooleanOp::Union => {
            // A outside B + B outside A
            builder.add_mesh_triangles(a, &a_outside);
            builder.add_mesh_triangles(b, &b_outside);
        }
        BooleanOp::Subtract => {
            // A outside B + B inside A (flipped)
            builder.add_mesh_triangles(a, &a_outside);
            builder.add_mesh_triangles_flipped(b, &b_inside);
        }
        BooleanOp::Intersect => {
            // A inside B + B inside A
            builder.add_mesh_triangles(a, &a_inside);
            builder.add_mesh_triangles(b, &b_inside);
        }
    }

    builder.build()
}

// -----------------------------------------------------------------------
// BSP Tree
// -----------------------------------------------------------------------

struct BspNode {
    plane_normal: Vec3,
    plane_dist: f32,
    front: Option<Box<BspNode>>,
    back: Option<Box<BspNode>>,
    triangles: Vec<[Vec3; 3]>,
}

struct BspTree {
    root: Option<BspNode>,
}

impl BspTree {
    fn build(mesh: &EditMesh) -> Self {
        let tris: Vec<[Vec3; 3]> = mesh
            .triangles
            .iter()
            .map(|t| {
                [
                    mesh.positions[t[0] as usize],
                    mesh.positions[t[1] as usize],
                    mesh.positions[t[2] as usize],
                ]
            })
            .collect();

        BspTree {
            root: BspNode::build(&tris),
        }
    }

    /// Classify a point as inside (true) or outside (false) the BSP solid.
    fn is_inside(&self, point: Vec3) -> bool {
        match &self.root {
            Some(node) => node.classify_point(point),
            None => false,
        }
    }
}

const BSP_EPSILON: f32 = 1e-5;

impl BspNode {
    fn build(triangles: &[[Vec3; 3]]) -> Option<BspNode> {
        if triangles.is_empty() {
            return None;
        }

        // Use first triangle as splitting plane
        let plane_tri = triangles[0];
        let edge1 = plane_tri[1] - plane_tri[0];
        let edge2 = plane_tri[2] - plane_tri[0];
        let plane_normal = edge1.cross(edge2).normalize_or_zero();
        if plane_normal == Vec3::ZERO {
            // Degenerate triangle — skip
            return BspNode::build(&triangles[1..]);
        }
        let plane_dist = plane_normal.dot(plane_tri[0]);

        let coplanar = vec![plane_tri];
        let mut front_tris = Vec::new();
        let mut back_tris = Vec::new();

        for tri in &triangles[1..] {
            let d0 = plane_normal.dot(tri[0]) - plane_dist;
            let d1 = plane_normal.dot(tri[1]) - plane_dist;
            let d2 = plane_normal.dot(tri[2]) - plane_dist;

            let s0 = classify_dist(d0);
            let s1 = classify_dist(d1);
            let s2 = classify_dist(d2);

            if s0 >= 0 && s1 >= 0 && s2 >= 0 {
                front_tris.push(*tri);
            } else if s0 <= 0 && s1 <= 0 && s2 <= 0 {
                back_tris.push(*tri);
            } else {
                // Triangle straddles the plane — split it
                let (mut f, mut b) = split_tri_by_plane(tri, plane_normal, plane_dist);
                front_tris.append(&mut f);
                back_tris.append(&mut b);
            }
        }

        Some(BspNode {
            plane_normal,
            plane_dist,
            front: BspNode::build(&front_tris).map(Box::new),
            back: BspNode::build(&back_tris).map(Box::new),
            triangles: coplanar,
        })
    }

    fn classify_point(&self, point: Vec3) -> bool {
        let d = self.plane_normal.dot(point) - self.plane_dist;
        if d > BSP_EPSILON {
            match &self.front {
                Some(node) => node.classify_point(point),
                None => false, // Outside
            }
        } else if d < -BSP_EPSILON {
            match &self.back {
                Some(node) => node.classify_point(point),
                None => true, // Inside
            }
        } else {
            // On plane — check back
            match &self.back {
                Some(node) => node.classify_point(point),
                None => true,
            }
        }
    }
}

fn classify_dist(d: f32) -> i32 {
    if d > BSP_EPSILON {
        1
    } else if d < -BSP_EPSILON {
        -1
    } else {
        0
    }
}

/// Split a triangle by a plane, returning (front_tris, back_tris).
fn split_tri_by_plane(
    tri: &[Vec3; 3],
    plane_normal: Vec3,
    plane_dist: f32,
) -> (Vec<[Vec3; 3]>, Vec<[Vec3; 3]>) {
    let dists = [
        plane_normal.dot(tri[0]) - plane_dist,
        plane_normal.dot(tri[1]) - plane_dist,
        plane_normal.dot(tri[2]) - plane_dist,
    ];

    let mut front_pts = Vec::new();
    let mut back_pts = Vec::new();

    for i in 0..3 {
        let j = (i + 1) % 3;
        let di = dists[i];
        let dj = dists[j];

        if di >= -BSP_EPSILON {
            front_pts.push(tri[i]);
        }
        if di <= BSP_EPSILON {
            back_pts.push(tri[i]);
        }

        if (di > BSP_EPSILON && dj < -BSP_EPSILON) || (di < -BSP_EPSILON && dj > BSP_EPSILON) {
            let t = di / (di - dj);
            let intersection = tri[i].lerp(tri[j], t);
            front_pts.push(intersection);
            back_pts.push(intersection);
        }
    }

    let front_tris = fan_triangulate(&front_pts);
    let back_tris = fan_triangulate(&back_pts);

    (front_tris, back_tris)
}

fn fan_triangulate(pts: &[Vec3]) -> Vec<[Vec3; 3]> {
    if pts.len() < 3 {
        return Vec::new();
    }
    let mut tris = Vec::new();
    for i in 1..pts.len() - 1 {
        tris.push([pts[0], pts[i], pts[i + 1]]);
    }
    tris
}

// -----------------------------------------------------------------------
// Triangle classification
// -----------------------------------------------------------------------

/// Classify each triangle of a mesh as inside or outside a BSP tree.
///
/// Returns (inside_indices, outside_indices).
fn classify_triangles(mesh: &EditMesh, tree: &BspTree) -> (Vec<usize>, Vec<usize>) {
    let mut inside = Vec::new();
    let mut outside = Vec::new();

    for (ti, tri) in mesh.triangles.iter().enumerate() {
        let center = (mesh.positions[tri[0] as usize]
            + mesh.positions[tri[1] as usize]
            + mesh.positions[tri[2] as usize])
            / 3.0;

        if tree.is_inside(center) {
            inside.push(ti);
        } else {
            outside.push(ti);
        }
    }

    (inside, outside)
}

// -----------------------------------------------------------------------
// Mesh builder
// -----------------------------------------------------------------------

struct MeshBuilder {
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    uvs: Vec<Vec2>,
    triangles: Vec<[u32; 3]>,
}

impl MeshBuilder {
    fn new() -> Self {
        Self {
            positions: Vec::new(),
            normals: Vec::new(),
            uvs: Vec::new(),
            triangles: Vec::new(),
        }
    }

    fn add_vertex(&mut self, pos: Vec3, nor: Vec3, uv: Vec2) -> u32 {
        let idx = self.positions.len() as u32;
        self.positions.push(pos);
        self.normals.push(nor);
        self.uvs.push(uv);
        idx
    }

    fn add_mesh_triangles(&mut self, mesh: &EditMesh, indices: &[usize]) {
        // Add all vertices (simple approach — could deduplicate but not critical for correctness)
        let mut vert_map: HashMap<u32, u32> = HashMap::new();
        for &ti in indices {
            let tri = mesh.triangles[ti];
            for &v in &tri {
                vert_map.entry(v).or_insert_with(|| {
                    self.add_vertex(
                        mesh.positions[v as usize],
                        mesh.normals[v as usize],
                        mesh.uvs[v as usize],
                    )
                });
            }
        }

        for &ti in indices {
            let tri = mesh.triangles[ti];
            self.triangles.push([
                vert_map[&tri[0]],
                vert_map[&tri[1]],
                vert_map[&tri[2]],
            ]);
        }
    }

    fn add_mesh_triangles_flipped(&mut self, mesh: &EditMesh, indices: &[usize]) {
        let mut vert_map: HashMap<u32, u32> = HashMap::new();
        for &ti in indices {
            let tri = mesh.triangles[ti];
            for &v in &tri {
                vert_map.entry(v).or_insert_with(|| {
                    self.add_vertex(
                        mesh.positions[v as usize],
                        -mesh.normals[v as usize], // Flip normals
                        mesh.uvs[v as usize],
                    )
                });
            }
        }

        for &ti in indices {
            let tri = mesh.triangles[ti];
            // Reverse winding order
            self.triangles.push([
                vert_map[&tri[0]],
                vert_map[&tri[2]],
                vert_map[&tri[1]],
            ]);
        }
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
