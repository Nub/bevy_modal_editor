//! Catmull-Clark subdivision surface.
//!
//! Produces a smooth subdivision by computing face points, edge points, and
//! updating vertex positions according to the Catmull-Clark rules.
//! Since our mesh is triangle-based, each triangle is split into 3 quads
//! (6 triangles).

use bevy::prelude::*;
use std::collections::HashMap;

use super::edit_mesh::{Edge, EditMesh};

/// Apply one level of Catmull-Clark subdivision.
///
/// Each triangle is split into 3 quads (6 triangles). Face points, edge points,
/// and updated vertex positions are computed per the Catmull-Clark rules.
pub fn catmull_clark_subdivide(mesh: &EditMesh) -> EditMesh {
    let adj = mesh.build_adjacency();

    // Step 1: Compute face points (centroid of each face)
    let face_points: Vec<Vec3> = (0..mesh.triangles.len())
        .map(|fi| mesh.face_center(fi))
        .collect();

    let face_uvs: Vec<Vec2> = (0..mesh.triangles.len())
        .map(|fi| mesh.face_uv_center(fi))
        .collect();

    // Step 2: Compute edge points
    // For interior edges: (avg of 2 endpoints + avg of 2 adjacent face points) / 2
    // For boundary edges: midpoint of the 2 endpoints
    let mut edge_points: HashMap<Edge, Vec3> = HashMap::new();
    let mut edge_uvs: HashMap<Edge, Vec2> = HashMap::new();

    for (edge, faces) in &adj {
        let v0 = mesh.positions[edge.0 as usize];
        let v1 = mesh.positions[edge.1 as usize];
        let uv0 = mesh.uvs[edge.0 as usize];
        let uv1 = mesh.uvs[edge.1 as usize];

        if faces.len() == 2 {
            // Interior edge
            let fp_avg = (face_points[faces[0]] + face_points[faces[1]]) * 0.5;
            let ep = ((v0 + v1) * 0.5 + fp_avg) * 0.5;
            edge_points.insert(*edge, ep);

            let fuv_avg = (face_uvs[faces[0]] + face_uvs[faces[1]]) * 0.5;
            edge_uvs.insert(*edge, (uv0 + uv1) * 0.25 + fuv_avg * 0.5);
        } else {
            // Boundary edge: simple midpoint
            edge_points.insert(*edge, (v0 + v1) * 0.5);
            edge_uvs.insert(*edge, (uv0 + uv1) * 0.5);
        }
    }

    // Step 3: Compute updated vertex positions
    // For interior vertices: (F + 2R + (n-3)P) / n
    //   where F = avg of adjacent face points, R = avg of adjacent edge midpoints,
    //   P = original position, n = valence
    // For boundary vertices: (E + 6P + E') / 8 where E, E' are boundary edge midpoints
    let mut new_vertex_positions: Vec<Vec3> = vec![Vec3::ZERO; mesh.positions.len()];
    let mut new_vertex_uvs: Vec<Vec2> = vec![Vec2::ZERO; mesh.positions.len()];

    // Determine boundary vertices
    let mut boundary_edges_per_vertex: HashMap<u32, Vec<Edge>> = HashMap::new();
    for (edge, faces) in &adj {
        if faces.len() == 1 {
            boundary_edges_per_vertex
                .entry(edge.0)
                .or_default()
                .push(*edge);
            boundary_edges_per_vertex
                .entry(edge.1)
                .or_default()
                .push(*edge);
        }
    }

    // Build vertex-to-faces map
    let mut vertex_faces: HashMap<u32, Vec<usize>> = HashMap::new();
    for (fi, tri) in mesh.triangles.iter().enumerate() {
        for &vi in tri {
            vertex_faces.entry(vi).or_default().push(fi);
        }
    }

    // Build vertex-to-edges map
    let mut vertex_edges: HashMap<u32, Vec<Edge>> = HashMap::new();
    for edge in adj.keys() {
        vertex_edges.entry(edge.0).or_default().push(*edge);
        vertex_edges.entry(edge.1).or_default().push(*edge);
    }

    for vi in 0..mesh.positions.len() {
        let vi32 = vi as u32;
        let p = mesh.positions[vi];
        let uv = mesh.uvs[vi];

        if let Some(bedges) = boundary_edges_per_vertex.get(&vi32) {
            // Boundary vertex: average of boundary edge midpoints and original
            if bedges.len() >= 2 {
                let e0 = edge_points.get(&bedges[0]).copied().unwrap_or(p);
                let e1 = edge_points.get(&bedges[1]).copied().unwrap_or(p);
                new_vertex_positions[vi] = (e0 + e1 + p * 6.0) / 8.0;

                let euv0 = edge_uvs.get(&bedges[0]).copied().unwrap_or(uv);
                let euv1 = edge_uvs.get(&bedges[1]).copied().unwrap_or(uv);
                new_vertex_uvs[vi] = (euv0 + euv1 + uv * 6.0) / 8.0;
            } else {
                new_vertex_positions[vi] = p;
                new_vertex_uvs[vi] = uv;
            }
        } else {
            // Interior vertex
            let faces = vertex_faces.get(&vi32);
            let edges = vertex_edges.get(&vi32);

            let n = faces.map(|f| f.len()).unwrap_or(0) as f32;
            if n < 1.0 {
                new_vertex_positions[vi] = p;
                new_vertex_uvs[vi] = uv;
                continue;
            }

            // F = average of adjacent face points
            let f_avg = faces
                .map(|fs| {
                    fs.iter()
                        .map(|&fi| face_points[fi])
                        .sum::<Vec3>()
                        / n
                })
                .unwrap_or(Vec3::ZERO);

            // R = average of adjacent edge midpoints
            let r_avg = edges
                .map(|es| {
                    let mid_sum: Vec3 = es
                        .iter()
                        .map(|e| {
                            let v0 = mesh.positions[e.0 as usize];
                            let v1 = mesh.positions[e.1 as usize];
                            (v0 + v1) * 0.5
                        })
                        .sum();
                    mid_sum / es.len() as f32
                })
                .unwrap_or(Vec3::ZERO);

            new_vertex_positions[vi] = (f_avg + r_avg * 2.0 + p * (n - 3.0)) / n;

            // UV: simple weighted average (not strictly CC but reasonable)
            let f_uv = faces
                .map(|fs| {
                    fs.iter()
                        .map(|&fi| face_uvs[fi])
                        .sum::<Vec2>()
                        / n
                })
                .unwrap_or(Vec2::ZERO);
            new_vertex_uvs[vi] = (f_uv + uv * (n - 1.0)) / n;
        }
    }

    // Step 4: Build new mesh
    // Each triangle [a, b, c] with face point FP and edge points EP_ab, EP_bc, EP_ca
    // becomes 3 quads (6 triangles):
    //   Quad 1: a', EP_ab, FP, EP_ca
    //   Quad 2: b', EP_bc, FP, EP_ab
    //   Quad 3: c', EP_ca, FP, EP_bc
    let mut new_positions = Vec::new();
    let mut new_normals = Vec::new();
    let mut new_uvs = Vec::new();
    let mut new_triangles = Vec::new();

    // Helper: add a vertex and return its index
    let mut add_vert = |pos: Vec3, uv: Vec2| -> u32 {
        let idx = new_positions.len() as u32;
        new_positions.push(pos);
        new_normals.push(Vec3::ZERO);
        new_uvs.push(uv);
        idx
    };

    for (fi, tri) in mesh.triangles.iter().enumerate() {
        let [a, b, c] = *tri;
        let fp_pos = face_points[fi];
        let fp_uv = face_uvs[fi];

        let e_ab = Edge::new(a, b);
        let e_bc = Edge::new(b, c);
        let e_ca = Edge::new(c, a);

        let va = add_vert(new_vertex_positions[a as usize], new_vertex_uvs[a as usize]);
        let vb = add_vert(new_vertex_positions[b as usize], new_vertex_uvs[b as usize]);
        let vc = add_vert(new_vertex_positions[c as usize], new_vertex_uvs[c as usize]);
        let vfp = add_vert(fp_pos, fp_uv);
        let vep_ab = add_vert(
            edge_points.get(&e_ab).copied().unwrap_or((mesh.positions[a as usize] + mesh.positions[b as usize]) * 0.5),
            edge_uvs.get(&e_ab).copied().unwrap_or((mesh.uvs[a as usize] + mesh.uvs[b as usize]) * 0.5),
        );
        let vep_bc = add_vert(
            edge_points.get(&e_bc).copied().unwrap_or((mesh.positions[b as usize] + mesh.positions[c as usize]) * 0.5),
            edge_uvs.get(&e_bc).copied().unwrap_or((mesh.uvs[b as usize] + mesh.uvs[c as usize]) * 0.5),
        );
        let vep_ca = add_vert(
            edge_points.get(&e_ca).copied().unwrap_or((mesh.positions[c as usize] + mesh.positions[a as usize]) * 0.5),
            edge_uvs.get(&e_ca).copied().unwrap_or((mesh.uvs[c as usize] + mesh.uvs[a as usize]) * 0.5),
        );

        // Quad around vertex a: va, vep_ab, vfp, vep_ca
        new_triangles.push([va, vep_ab, vfp]);
        new_triangles.push([va, vfp, vep_ca]);

        // Quad around vertex b: vb, vep_bc, vfp, vep_ab
        new_triangles.push([vb, vep_bc, vfp]);
        new_triangles.push([vb, vfp, vep_ab]);

        // Quad around vertex c: vc, vep_ca, vfp, vep_bc
        new_triangles.push([vc, vep_ca, vfp]);
        new_triangles.push([vc, vfp, vep_bc]);
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
