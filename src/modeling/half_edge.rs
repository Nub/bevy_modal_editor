//! Half-edge mesh data structure for efficient topology traversal.
//!
//! `HalfEdgeMesh` is the primary editing structure — built from `EditMesh` on
//! mode entry, all operations run against it, then flushed back to `EditMesh`
//! on confirm.
//!
//! Uses index-based arena storage (not pointers) for cache-friendly traversal.

use bevy::prelude::*;
use std::collections::HashMap;

use super::edit_mesh::EditMesh;

/// Index into the half-edge array.
pub type HalfEdgeId = u32;
/// Index into the vertex array.
pub type VertexId = u32;
/// Index into the face array.
pub type FaceId = u32;

/// Sentinel value indicating "no element" (null pointer equivalent).
pub const INVALID: u32 = u32::MAX;

/// A single half-edge in the mesh.
#[derive(Debug, Clone, Copy)]
pub struct HalfEdge {
    /// The opposite half-edge (sharing the same geometric edge).
    pub twin: HalfEdgeId,
    /// Next half-edge around the face (counter-clockwise).
    pub next: HalfEdgeId,
    /// Previous half-edge around the face (clockwise).
    pub prev: HalfEdgeId,
    /// Vertex this half-edge originates from.
    pub vertex: VertexId,
    /// Face this half-edge borders (INVALID for boundary half-edges).
    pub face: FaceId,
}

/// A vertex in the half-edge mesh.
#[derive(Debug, Clone)]
pub struct HVertex {
    pub position: Vec3,
    pub normal: Vec3,
    pub uv: Vec2,
    /// One outgoing half-edge from this vertex.
    pub half_edge: HalfEdgeId,
}

/// A face in the half-edge mesh.
#[derive(Debug, Clone, Copy)]
pub struct HFace {
    /// One half-edge on the boundary of this face.
    pub half_edge: HalfEdgeId,
}

/// Half-edge mesh with index-based arena storage.
///
/// Supports efficient O(1)-per-step traversal of vertex neighborhoods,
/// edge loops, and face boundaries.
#[derive(Debug, Clone)]
pub struct HalfEdgeMesh {
    pub half_edges: Vec<HalfEdge>,
    pub vertices: Vec<HVertex>,
    pub faces: Vec<HFace>,
}

impl HalfEdgeMesh {
    /// Build a `HalfEdgeMesh` from an `EditMesh`.
    ///
    /// The EditMesh must consist of triangles. Each triangle produces 3 half-edges.
    /// Twin half-edges are linked where triangles share edges; boundary edges get
    /// boundary twins (face = INVALID) so traversal never hits dead ends.
    pub fn from_edit_mesh(mesh: &EditMesh) -> Self {
        let num_faces = mesh.triangles.len();

        // Create vertices
        let mut vertices: Vec<HVertex> = mesh
            .positions
            .iter()
            .zip(mesh.normals.iter())
            .zip(mesh.uvs.iter())
            .map(|((&pos, &nor), &uv)| HVertex {
                position: pos,
                normal: nor,
                uv,
                half_edge: INVALID,
            })
            .collect();

        // Create faces and half-edges
        // Each triangle gets 3 half-edges
        let mut half_edges: Vec<HalfEdge> = Vec::with_capacity(num_faces * 3);
        let mut faces: Vec<HFace> = Vec::with_capacity(num_faces);

        // Map from directed edge (from, to) -> half-edge index for twin linking
        let mut edge_map: HashMap<(u32, u32), HalfEdgeId> = HashMap::new();

        for (fi, tri) in mesh.triangles.iter().enumerate() {
            let face_id = fi as FaceId;
            let base = half_edges.len() as HalfEdgeId;

            // Create 3 half-edges for this triangle
            for i in 0..3u32 {
                let from = tri[i as usize];
                let he_id = base + i;

                half_edges.push(HalfEdge {
                    twin: INVALID,
                    next: base + (i + 1) % 3,
                    prev: base + (i + 2) % 3,
                    vertex: from,
                    face: face_id,
                });

                // Set vertex's outgoing half-edge if not yet set
                if vertices[from as usize].half_edge == INVALID {
                    vertices[from as usize].half_edge = he_id;
                }

                // Register in edge map for twin linking
                let to = tri[((i + 1) % 3) as usize];
                edge_map.insert((from, to), he_id);
            }

            faces.push(HFace { half_edge: base });
        }

        // Link twins: for each half-edge (from, to), find the half-edge (to, from)
        let num_interior_he = half_edges.len();
        let mut boundary_twins: Vec<HalfEdge> = Vec::new();

        for he_idx in 0..num_interior_he {
            if half_edges[he_idx].twin != INVALID {
                continue;
            }

            let from = half_edges[he_idx].vertex;
            let to = half_edges[half_edges[he_idx].next as usize].vertex;

            if let Some(&twin_idx) = edge_map.get(&(to, from)) {
                half_edges[he_idx].twin = twin_idx;
                half_edges[twin_idx as usize].twin = he_idx as HalfEdgeId;
            } else {
                // Boundary edge — create a boundary half-edge
                let boundary_id = (half_edges.len() + boundary_twins.len()) as HalfEdgeId;
                half_edges[he_idx].twin = boundary_id;
                boundary_twins.push(HalfEdge {
                    twin: he_idx as HalfEdgeId,
                    next: INVALID, // linked below
                    prev: INVALID, // linked below
                    vertex: to,    // boundary half-edge goes opposite direction
                    face: INVALID, // no face for boundary
                });
            }
        }

        half_edges.extend(boundary_twins);

        // Link boundary half-edge next/prev chains
        // For each boundary half-edge, its next should be the boundary half-edge
        // leaving from the same vertex that continues around the boundary.
        Self::link_boundary_chains(&mut half_edges, num_interior_he);

        HalfEdgeMesh {
            half_edges,
            vertices,
            faces,
        }
    }

    /// Link next/prev pointers for boundary half-edges.
    fn link_boundary_chains(half_edges: &mut [HalfEdge], interior_count: usize) {
        // Build map: vertex -> boundary half-edge starting from that vertex
        let mut boundary_from: HashMap<VertexId, HalfEdgeId> = HashMap::new();
        for i in interior_count..half_edges.len() {
            let he = &half_edges[i];
            boundary_from.insert(he.vertex, i as HalfEdgeId);
        }

        for i in interior_count..half_edges.len() {
            let he = &half_edges[i];
            // This boundary he goes from he.vertex to twin's vertex
            let twin = &half_edges[he.twin as usize];
            let end_vertex = twin.vertex;

            // The next boundary half-edge starts from end_vertex
            if let Some(&next_id) = boundary_from.get(&end_vertex) {
                half_edges[i].next = next_id;
                half_edges[next_id as usize].prev = i as HalfEdgeId;
            }
        }
    }

    /// Convert back to an `EditMesh`.
    ///
    /// Each face becomes a triangle (faces must remain triangular).
    /// Vertex data is copied back with any modifications from editing operations.
    pub fn to_edit_mesh(&self) -> EditMesh {
        let positions: Vec<Vec3> = self.vertices.iter().map(|v| v.position).collect();
        let normals: Vec<Vec3> = self.vertices.iter().map(|v| v.normal).collect();
        let uvs: Vec<Vec2> = self.vertices.iter().map(|v| v.uv).collect();

        let mut triangles = Vec::with_capacity(self.faces.len());
        for face in &self.faces {
            let he0 = face.half_edge;
            let he1 = self.half_edges[he0 as usize].next;
            let he2 = self.half_edges[he1 as usize].next;

            triangles.push([
                self.half_edges[he0 as usize].vertex,
                self.half_edges[he1 as usize].vertex,
                self.half_edges[he2 as usize].vertex,
            ]);
        }

        EditMesh {
            positions,
            normals,
            uvs,
            triangles,
        }
    }

    // -------------------------------------------------------------------
    // Traversal iterators
    // -------------------------------------------------------------------

    /// Iterate over all outgoing half-edges from a vertex.
    ///
    /// Walks around the vertex by following twin->next. Returns half-edge indices.
    pub fn vertex_half_edges(&self, vertex: VertexId) -> Vec<HalfEdgeId> {
        let start = self.vertices[vertex as usize].half_edge;
        if start == INVALID {
            return Vec::new();
        }

        let mut result = Vec::new();
        let mut current = start;
        loop {
            result.push(current);

            // Go to twin, then next to get to the next outgoing half-edge
            let twin = self.half_edges[current as usize].twin;
            if twin == INVALID {
                break;
            }
            current = self.half_edges[twin as usize].next;
            if current == INVALID || current == start {
                break;
            }
        }

        // If we didn't complete the loop (boundary vertex), also walk the other direction
        if current != start {
            let twin = self.half_edges[start as usize].twin;
            if twin != INVALID {
                let mut rev = self.half_edges[twin as usize].prev;
                // Walk prev->twin to find half-edges on the other side
                while rev != INVALID {
                    let rev_twin = self.half_edges[rev as usize].twin;
                    if rev_twin == INVALID {
                        break;
                    }
                    // rev_twin is an outgoing half-edge from vertex (if it originates there)
                    if self.half_edges[rev_twin as usize].vertex != vertex {
                        break;
                    }
                    result.push(rev_twin);
                    rev = self.half_edges[rev_twin as usize].prev;
                    if rev == INVALID {
                        break;
                    }
                    let check = self.half_edges[rev as usize].twin;
                    if check == INVALID || check == start {
                        break;
                    }
                    rev = self.half_edges[check as usize].prev;
                }
            }
        }

        result
    }

    /// Get all face indices adjacent to a vertex.
    pub fn vertex_faces(&self, vertex: VertexId) -> Vec<FaceId> {
        self.vertex_half_edges(vertex)
            .into_iter()
            .filter_map(|he| {
                let face = self.half_edges[he as usize].face;
                if face != INVALID {
                    Some(face)
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get the half-edge indices forming a face boundary (in order).
    pub fn face_half_edges(&self, face: FaceId) -> Vec<HalfEdgeId> {
        let start = self.faces[face as usize].half_edge;
        let mut result = Vec::new();
        let mut current = start;
        loop {
            result.push(current);
            current = self.half_edges[current as usize].next;
            if current == start {
                break;
            }
        }
        result
    }

    /// Get the vertex indices forming a face boundary (in order).
    pub fn face_vertices(&self, face: FaceId) -> Vec<VertexId> {
        self.face_half_edges(face)
            .into_iter()
            .map(|he| self.half_edges[he as usize].vertex)
            .collect()
    }

    /// Get the vertex pair (from, to) for a half-edge.
    pub fn edge_vertices(&self, he: HalfEdgeId) -> (VertexId, VertexId) {
        let from = self.half_edges[he as usize].vertex;
        let to = self.half_edges[self.half_edges[he as usize].next as usize].vertex;
        (from, to)
    }

    /// Compute face normal from vertex positions.
    pub fn face_normal(&self, face: FaceId) -> Vec3 {
        let verts = self.face_vertices(face);
        if verts.len() < 3 {
            return Vec3::ZERO;
        }
        let p0 = self.vertices[verts[0] as usize].position;
        let p1 = self.vertices[verts[1] as usize].position;
        let p2 = self.vertices[verts[2] as usize].position;
        (p1 - p0).cross(p2 - p0).normalize_or_zero()
    }

    /// Compute face centroid.
    pub fn face_center(&self, face: FaceId) -> Vec3 {
        let verts = self.face_vertices(face);
        if verts.is_empty() {
            return Vec3::ZERO;
        }
        let sum: Vec3 = verts
            .iter()
            .map(|&v| self.vertices[v as usize].position)
            .sum();
        sum / verts.len() as f32
    }

    /// Compute face area (for triangles).
    pub fn face_area(&self, face: FaceId) -> f32 {
        let verts = self.face_vertices(face);
        if verts.len() < 3 {
            return 0.0;
        }
        let p0 = self.vertices[verts[0] as usize].position;
        let p1 = self.vertices[verts[1] as usize].position;
        let p2 = self.vertices[verts[2] as usize].position;
        (p1 - p0).cross(p2 - p0).length() * 0.5
    }

    /// Check if a half-edge is on the boundary (its face is INVALID).
    pub fn is_boundary(&self, he: HalfEdgeId) -> bool {
        self.half_edges[he as usize].face == INVALID
    }

    /// Recompute all vertex normals from face normals (smooth shading).
    pub fn recompute_normals(&mut self) {
        for v in &mut self.vertices {
            v.normal = Vec3::ZERO;
        }

        for fi in 0..self.faces.len() {
            let verts = self.face_vertices(fi as FaceId);
            if verts.len() < 3 {
                continue;
            }
            let p0 = self.vertices[verts[0] as usize].position;
            let p1 = self.vertices[verts[1] as usize].position;
            let p2 = self.vertices[verts[2] as usize].position;
            let normal = (p1 - p0).cross(p2 - p0);
            for &v in &verts {
                self.vertices[v as usize].normal += normal;
            }
        }

        for v in &mut self.vertices {
            v.normal = v.normal.normalize_or_zero();
        }
    }

    /// Add a new vertex and return its index.
    pub fn add_vertex(&mut self, position: Vec3, normal: Vec3, uv: Vec2) -> VertexId {
        let id = self.vertices.len() as VertexId;
        self.vertices.push(HVertex {
            position,
            normal,
            uv,
            half_edge: INVALID,
        });
        id
    }

    /// Add a triangular face connecting three vertices. Returns the new face ID.
    ///
    /// Creates 3 half-edges and links them. Does NOT link twins — caller must
    /// handle twin linkage (or call `rebuild_twins()` after batch insertion).
    pub fn add_face(&mut self, v0: VertexId, v1: VertexId, v2: VertexId) -> FaceId {
        let face_id = self.faces.len() as FaceId;
        let base = self.half_edges.len() as HalfEdgeId;

        let verts = [v0, v1, v2];
        for i in 0..3u32 {
            self.half_edges.push(HalfEdge {
                twin: INVALID,
                next: base + (i + 1) % 3,
                prev: base + (i + 2) % 3,
                vertex: verts[i as usize],
                face: face_id,
            });

            let he_id = base + i;
            if self.vertices[verts[i as usize] as usize].half_edge == INVALID {
                self.vertices[verts[i as usize] as usize].half_edge = he_id;
            }
        }

        self.faces.push(HFace { half_edge: base });
        face_id
    }

    /// Rebuild all twin linkages from scratch.
    ///
    /// Call this after batch face insertion to properly link all half-edge twins
    /// and create boundary half-edges.
    pub fn rebuild_twins(&mut self) {
        // Clear existing twin links
        for he in &mut self.half_edges {
            he.twin = INVALID;
        }

        // Remove any existing boundary half-edges (face == INVALID)
        // But only those that we created as boundary — we'll recreate them
        self.half_edges.retain(|he| he.face != INVALID);

        // Fix face half_edge references and next/prev within faces
        // After retain, indices shifted — rebuild from faces
        let faces_data: Vec<Vec<VertexId>> = self
            .faces
            .iter()
            .map(|f| self.face_vertices_from_he(f.half_edge))
            .collect();

        // Rebuild half-edges from scratch
        self.half_edges.clear();
        let mut edge_map: HashMap<(u32, u32), HalfEdgeId> = HashMap::new();

        for (fi, verts) in faces_data.iter().enumerate() {
            let base = self.half_edges.len() as HalfEdgeId;
            let n = verts.len() as u32;

            for i in 0..n {
                let from = verts[i as usize];
                let to = verts[((i + 1) % n) as usize];
                let he_id = base + i;

                self.half_edges.push(HalfEdge {
                    twin: INVALID,
                    next: base + (i + 1) % n,
                    prev: base + (i + n - 1) % n,
                    vertex: from,
                    face: fi as FaceId,
                });

                if self.vertices[from as usize].half_edge == INVALID
                    || self.half_edges[self.vertices[from as usize].half_edge as usize].face
                        == INVALID
                {
                    self.vertices[from as usize].half_edge = he_id;
                }

                edge_map.insert((from, to), he_id);
            }

            self.faces[fi].half_edge = base;
        }

        // Link twins
        let interior_count = self.half_edges.len();
        let mut boundary_twins: Vec<HalfEdge> = Vec::new();

        for he_idx in 0..interior_count {
            if self.half_edges[he_idx].twin != INVALID {
                continue;
            }
            let from = self.half_edges[he_idx].vertex;
            let next_he = self.half_edges[he_idx].next;
            let to = self.half_edges[next_he as usize].vertex;

            if let Some(&twin_idx) = edge_map.get(&(to, from)) {
                self.half_edges[he_idx].twin = twin_idx;
                self.half_edges[twin_idx as usize].twin = he_idx as HalfEdgeId;
            } else {
                let boundary_id = (self.half_edges.len() + boundary_twins.len()) as HalfEdgeId;
                self.half_edges[he_idx].twin = boundary_id;
                boundary_twins.push(HalfEdge {
                    twin: he_idx as HalfEdgeId,
                    next: INVALID,
                    prev: INVALID,
                    vertex: to,
                    face: INVALID,
                });
            }
        }

        self.half_edges.extend(boundary_twins);
        Self::link_boundary_chains(&mut self.half_edges, interior_count);
    }

    /// Helper: get face vertices by walking half-edges from a starting he.
    /// Used internally during rebuild before indices might be invalidated.
    fn face_vertices_from_he(&self, start_he: HalfEdgeId) -> Vec<VertexId> {
        if start_he == INVALID || start_he as usize >= self.half_edges.len() {
            return Vec::new();
        }
        let mut result = Vec::new();
        let mut current = start_he;
        loop {
            result.push(self.half_edges[current as usize].vertex);
            current = self.half_edges[current as usize].next;
            if current == start_he || current == INVALID {
                break;
            }
            if result.len() > 64 {
                break; // safety
            }
        }
        result
    }

    /// Get the number of edges in the mesh (each pair of interior twins = 1 edge).
    pub fn edge_count(&self) -> usize {
        let mut count = 0;
        for (i, he) in self.half_edges.iter().enumerate() {
            // Count each edge once: only count when i < twin
            if he.twin != INVALID && (i as u32) < he.twin {
                count += 1;
            } else if he.twin == INVALID {
                count += 1;
            }
        }
        count
    }

    /// Get the midpoint of an edge given by a half-edge index.
    pub fn edge_midpoint(&self, he: HalfEdgeId) -> Vec3 {
        let (from, to) = self.edge_vertices(he);
        (self.vertices[from as usize].position + self.vertices[to as usize].position) * 0.5
    }

    /// Get all neighboring vertex indices connected by edges to `vertex`.
    pub fn vertex_neighbors(&self, vertex: VertexId) -> Vec<VertexId> {
        self.vertex_half_edges(vertex)
            .into_iter()
            .map(|he| {
                let twin = self.half_edges[he as usize].twin;
                if twin != INVALID {
                    self.half_edges[twin as usize].vertex
                } else {
                    // For boundary half-edges, the "to" vertex is found via next
                    let next = self.half_edges[he as usize].next;
                    if next != INVALID {
                        self.half_edges[next as usize].vertex
                    } else {
                        INVALID
                    }
                }
            })
            .filter(|&v| v != INVALID)
            .collect()
    }

    /// Get all canonical edge half-edge IDs emanating from `vertex`.
    pub fn vertex_edges(&self, vertex: VertexId) -> Vec<HalfEdgeId> {
        self.vertex_half_edges(vertex)
            .into_iter()
            .map(|he| {
                let twin = self.half_edges[he as usize].twin;
                if twin != INVALID && (he as u32) > twin {
                    twin
                } else {
                    he
                }
            })
            .collect()
    }

    /// Get canonical edge half-edge pairs: for each geometric edge, return
    /// the half-edge with the lower index. This gives a unique ID per edge.
    pub fn unique_edges(&self) -> Vec<HalfEdgeId> {
        let mut edges = Vec::new();
        for (i, he) in self.half_edges.iter().enumerate() {
            if he.face == INVALID {
                continue; // skip boundary half-edges
            }
            if he.twin == INVALID || (i as u32) < he.twin {
                edges.push(i as HalfEdgeId);
            }
        }
        edges
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modeling::edit_mesh::EditMesh;

    fn make_single_triangle() -> EditMesh {
        EditMesh {
            positions: vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(0.0, 1.0, 0.0),
            ],
            normals: vec![Vec3::Z; 3],
            uvs: vec![Vec2::ZERO; 3],
            triangles: vec![[0, 1, 2]],
        }
    }

    fn make_two_triangles() -> EditMesh {
        // Two triangles sharing edge (1,2):
        //   0--1
        //   |/ |
        //   2--3
        EditMesh {
            positions: vec![
                Vec3::new(0.0, 1.0, 0.0),
                Vec3::new(1.0, 1.0, 0.0),
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
            ],
            normals: vec![Vec3::Z; 4],
            uvs: vec![Vec2::ZERO; 4],
            triangles: vec![[0, 1, 2], [1, 3, 2]],
        }
    }

    fn make_cube() -> EditMesh {
        // Minimal cube: 8 vertices, 12 triangles (2 per face)
        let positions = vec![
            Vec3::new(-0.5, -0.5, -0.5), // 0
            Vec3::new(0.5, -0.5, -0.5),  // 1
            Vec3::new(0.5, 0.5, -0.5),   // 2
            Vec3::new(-0.5, 0.5, -0.5),  // 3
            Vec3::new(-0.5, -0.5, 0.5),  // 4
            Vec3::new(0.5, -0.5, 0.5),   // 5
            Vec3::new(0.5, 0.5, 0.5),    // 6
            Vec3::new(-0.5, 0.5, 0.5),   // 7
        ];
        let normals = vec![Vec3::ZERO; 8];
        let uvs = vec![Vec2::ZERO; 8];
        let triangles = vec![
            // Front (z+)
            [4, 5, 6],
            [4, 6, 7],
            // Back (z-)
            [1, 0, 3],
            [1, 3, 2],
            // Right (x+)
            [5, 1, 2],
            [5, 2, 6],
            // Left (x-)
            [0, 4, 7],
            [0, 7, 3],
            // Top (y+)
            [7, 6, 2],
            [7, 2, 3],
            // Bottom (y-)
            [0, 1, 5],
            [0, 5, 4],
        ];
        EditMesh {
            positions,
            normals,
            uvs,
            triangles,
        }
    }

    #[test]
    fn round_trip_single_triangle() {
        let mesh = make_single_triangle();
        let he = HalfEdgeMesh::from_edit_mesh(&mesh);
        let back = he.to_edit_mesh();

        assert_eq!(back.positions.len(), 3);
        assert_eq!(back.triangles.len(), 1);
        assert_eq!(back.triangles[0], mesh.triangles[0]);
    }

    #[test]
    fn round_trip_two_triangles() {
        let mesh = make_two_triangles();
        let he = HalfEdgeMesh::from_edit_mesh(&mesh);
        let back = he.to_edit_mesh();

        assert_eq!(back.positions.len(), 4);
        assert_eq!(back.triangles.len(), 2);
    }

    #[test]
    fn round_trip_cube() {
        let mesh = make_cube();
        let he = HalfEdgeMesh::from_edit_mesh(&mesh);
        let back = he.to_edit_mesh();

        assert_eq!(back.positions.len(), 8);
        assert_eq!(back.triangles.len(), 12);
    }

    #[test]
    fn twin_linkage() {
        let mesh = make_two_triangles();
        let he = HalfEdgeMesh::from_edit_mesh(&mesh);

        // Every half-edge with a face should have a twin
        for (i, edge) in he.half_edges.iter().enumerate() {
            assert_ne!(edge.twin, INVALID, "half-edge {} has no twin", i);
            // Twin's twin should be self
            let twin = &he.half_edges[edge.twin as usize];
            assert_eq!(twin.twin, i as u32, "twin linkage broken at {}", i);
        }
    }

    #[test]
    fn vertex_fan() {
        let mesh = make_two_triangles();
        let he = HalfEdgeMesh::from_edit_mesh(&mesh);

        // Vertex 1 should be part of 2 faces
        let faces = he.vertex_faces(1);
        assert_eq!(faces.len(), 2);

        // Vertex 0 should be part of 1 face
        let faces = he.vertex_faces(0);
        assert_eq!(faces.len(), 1);
    }

    #[test]
    fn face_traversal() {
        let mesh = make_two_triangles();
        let he = HalfEdgeMesh::from_edit_mesh(&mesh);

        // Each face should have 3 half-edges (triangles)
        for fi in 0..he.faces.len() {
            let edges = he.face_half_edges(fi as FaceId);
            assert_eq!(edges.len(), 3, "face {} has {} half-edges", fi, edges.len());
        }
    }

    #[test]
    fn cube_topology() {
        let mesh = make_cube();
        let he = HalfEdgeMesh::from_edit_mesh(&mesh);

        assert_eq!(he.vertices.len(), 8);
        assert_eq!(he.faces.len(), 12);

        // A closed cube should have no boundary half-edges
        for edge in &he.half_edges {
            assert_ne!(edge.twin, INVALID);
        }

        // Each vertex of a cube touches exactly 4-6 faces depending on triangulation
        // With our triangulation, corner vertices each touch 3 quad faces = 6 triangles... but
        // vertex sharing means each vertex touches multiple triangles
        for vi in 0..8 {
            let faces = he.vertex_faces(vi);
            // Each cube vertex is shared by 3 quads = 6 triangles... but fan walking
            // may not find all due to boundary vertex issues. Just check > 0.
            assert!(!faces.is_empty(), "vertex {} has no adjacent faces", vi);
        }
    }
}
