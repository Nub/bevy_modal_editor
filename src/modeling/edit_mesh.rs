//! Core mesh data structure for the mesh modeling tool.
//!
//! `EditMesh` is an indexed face list — a thin wrapper around Bevy mesh data.
//! It supports face queries, adjacency building, and boundary edge detection
//! needed for extrusion and cutting.

use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

/// Index of a triangle face in the mesh.
pub type FaceIndex = usize;

/// Canonical edge representation (lower vertex index first).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Edge(pub u32, pub u32);

impl Edge {
    /// Create a canonical edge with the lower index first.
    pub fn new(a: u32, b: u32) -> Self {
        if a <= b { Edge(a, b) } else { Edge(b, a) }
    }
}

/// Indexed triangle mesh suitable for face-level editing.
#[derive(Debug, Clone)]
pub struct EditMesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub triangles: Vec<[u32; 3]>,
}

impl EditMesh {
    /// Build an `EditMesh` from a Bevy `Mesh`.
    ///
    /// Returns `None` if the mesh lacks positions or uses a non-triangle topology.
    pub fn from_bevy_mesh(mesh: &Mesh) -> Option<Self> {
        if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
            return None;
        }

        let positions: Vec<Vec3> = match mesh.attribute(Mesh::ATTRIBUTE_POSITION)? {
            VertexAttributeValues::Float32x3(v) => v.iter().map(|p| Vec3::from(*p)).collect(),
            _ => return None,
        };

        let normals: Vec<Vec3> = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
            Some(VertexAttributeValues::Float32x3(v)) => {
                v.iter().map(|n| Vec3::from(*n)).collect()
            }
            _ => vec![Vec3::ZERO; positions.len()],
        };

        let uvs: Vec<Vec2> = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
            Some(VertexAttributeValues::Float32x2(v)) => {
                v.iter().map(|u| Vec2::from(*u)).collect()
            }
            _ => vec![Vec2::ZERO; positions.len()],
        };

        let triangles = match mesh.indices() {
            Some(Indices::U32(indices)) => indices
                .chunks(3)
                .map(|c| [c[0], c[1], c[2]])
                .collect(),
            Some(Indices::U16(indices)) => indices
                .chunks(3)
                .map(|c| [c[0] as u32, c[1] as u32, c[2] as u32])
                .collect(),
            None => {
                // Non-indexed: generate sequential indices
                (0..positions.len() as u32)
                    .collect::<Vec<_>>()
                    .chunks(3)
                    .map(|c| [c[0], c[1], c[2]])
                    .collect()
            }
        };

        Some(EditMesh {
            positions,
            normals,
            uvs,
            triangles,
        })
    }

    /// Convert back to a Bevy `Mesh` with normals and tangents.
    pub fn to_bevy_mesh(&self) -> Mesh {
        let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, default());

        mesh.insert_attribute(
            Mesh::ATTRIBUTE_POSITION,
            self.positions.iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_NORMAL,
            self.normals.iter().map(|n| [n.x, n.y, n.z]).collect::<Vec<_>>(),
        );
        mesh.insert_attribute(
            Mesh::ATTRIBUTE_UV_0,
            self.uvs.iter().map(|u| [u.x, u.y]).collect::<Vec<_>>(),
        );

        let indices: Vec<u32> = self.triangles.iter().flat_map(|t| t.iter().copied()).collect();
        mesh.insert_indices(Indices::U32(indices));

        // Generate tangents for normal mapping support
        match mesh.with_generated_tangents() {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to generate tangents for EditMesh: {e}");
                // Rebuild mesh without tangents as fallback
                let mut fallback = Mesh::new(PrimitiveTopology::TriangleList, default());
                fallback.insert_attribute(
                    Mesh::ATTRIBUTE_POSITION,
                    self.positions.iter().map(|p| [p.x, p.y, p.z]).collect::<Vec<_>>(),
                );
                fallback.insert_attribute(
                    Mesh::ATTRIBUTE_NORMAL,
                    self.normals.iter().map(|n| [n.x, n.y, n.z]).collect::<Vec<_>>(),
                );
                fallback.insert_attribute(
                    Mesh::ATTRIBUTE_UV_0,
                    self.uvs.iter().map(|u| [u.x, u.y]).collect::<Vec<_>>(),
                );
                let indices: Vec<u32> = self.triangles.iter().flat_map(|t| t.iter().copied()).collect();
                fallback.insert_indices(Indices::U32(indices));
                fallback
            }
        }
    }

    /// Compute the face normal for a triangle.
    pub fn face_normal(&self, face: FaceIndex) -> Vec3 {
        let [a, b, c] = self.triangles[face];
        let v0 = self.positions[a as usize];
        let v1 = self.positions[b as usize];
        let v2 = self.positions[c as usize];
        (v1 - v0).cross(v2 - v0).normalize_or_zero()
    }

    /// Compute the centroid of a triangle face.
    pub fn face_center(&self, face: FaceIndex) -> Vec3 {
        let [a, b, c] = self.triangles[face];
        (self.positions[a as usize] + self.positions[b as usize] + self.positions[c as usize])
            / 3.0
    }

    /// Compute the area of a triangle face.
    pub fn face_area(&self, face: FaceIndex) -> f32 {
        let [a, b, c] = self.triangles[face];
        let v0 = self.positions[a as usize];
        let v1 = self.positions[b as usize];
        let v2 = self.positions[c as usize];
        (v1 - v0).cross(v2 - v0).length() * 0.5
    }

    /// Get the three edges of a triangle face.
    pub fn face_edges(&self, face: FaceIndex) -> [Edge; 3] {
        let [a, b, c] = self.triangles[face];
        [Edge::new(a, b), Edge::new(b, c), Edge::new(c, a)]
    }

    /// Average UV coordinate for a face.
    pub fn face_uv_center(&self, face: FaceIndex) -> Vec2 {
        let [a, b, c] = self.triangles[face];
        (self.uvs[a as usize] + self.uvs[b as usize] + self.uvs[c as usize]) / 3.0
    }

    /// Build adjacency: maps each edge to the faces that share it.
    pub fn build_adjacency(&self) -> HashMap<Edge, Vec<FaceIndex>> {
        let mut adj: HashMap<Edge, Vec<FaceIndex>> = HashMap::new();
        for (fi, tri) in self.triangles.iter().enumerate() {
            let edges = [
                Edge::new(tri[0], tri[1]),
                Edge::new(tri[1], tri[2]),
                Edge::new(tri[2], tri[0]),
            ];
            for edge in edges {
                adj.entry(edge).or_default().push(fi);
            }
        }
        adj
    }

    /// Find boundary edges of a face selection — edges shared by exactly one
    /// selected face and at least one unselected face (or only one face total).
    pub fn boundary_edges(&self, selected: &HashSet<FaceIndex>) -> Vec<Edge> {
        let adj = self.build_adjacency();
        let mut boundary = Vec::new();

        for (edge, faces) in &adj {
            let sel_count = faces.iter().filter(|f| selected.contains(f)).count();
            let unsel_count = faces.len() - sel_count;
            // Boundary: at least one selected and at least one unselected,
            // or exactly one face total that is selected (mesh boundary).
            if sel_count > 0 && (unsel_count > 0 || faces.len() == 1) {
                boundary.push(*edge);
            }
        }

        boundary
    }

    /// Collect all unique vertex indices used by the selected faces.
    pub fn selected_vertices(&self, selected: &HashSet<FaceIndex>) -> HashSet<u32> {
        let mut verts = HashSet::new();
        for &fi in selected {
            let [a, b, c] = self.triangles[fi];
            verts.insert(a);
            verts.insert(b);
            verts.insert(c);
        }
        verts
    }

    /// Collect all unique vertex indices that lie on boundary edges.
    pub fn boundary_vertices(&self, selected: &HashSet<FaceIndex>) -> HashSet<u32> {
        let boundary = self.boundary_edges(selected);
        let mut verts = HashSet::new();
        for edge in boundary {
            verts.insert(edge.0);
            verts.insert(edge.1);
        }
        verts
    }

    /// Recompute normals for all faces (flat shading).
    pub fn recompute_normals(&mut self) {
        // Zero out all normals
        for n in &mut self.normals {
            *n = Vec3::ZERO;
        }

        // Accumulate face normals to vertices
        for tri in &self.triangles {
            let v0 = self.positions[tri[0] as usize];
            let v1 = self.positions[tri[1] as usize];
            let v2 = self.positions[tri[2] as usize];
            let normal = (v1 - v0).cross(v2 - v0);
            self.normals[tri[0] as usize] += normal;
            self.normals[tri[1] as usize] += normal;
            self.normals[tri[2] as usize] += normal;
        }

        // Normalize
        for n in &mut self.normals {
            *n = n.normalize_or_zero();
        }
    }

    /// Number of triangle faces.
    pub fn face_count(&self) -> usize {
        self.triangles.len()
    }
}
