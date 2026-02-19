//! Face inset operation for the mesh modeling tool.
//!
//! Insets selected faces by creating smaller inner faces offset toward each
//! face's centroid, then bridging the gap with quad strips between inner and
//! outer boundaries.

use bevy::prelude::*;
use std::collections::HashSet;

use super::edit_mesh::{EditMesh, FaceIndex};

/// Inset each selected face individually by the given fraction (0.0 = no change, 1.0 = collapse to centroid).
///
/// For each selected triangle:
/// 1. Create 3 new "inner" vertices lerped toward the face centroid
/// 2. Replace the original triangle with the inner triangle
/// 3. Create 3 connecting quads (6 triangles) bridging inner to outer edges
///
/// Returns a new `EditMesh` with the inset applied.
pub fn inset_faces(
    mesh: &EditMesh,
    selected: &HashSet<FaceIndex>,
    inset_fraction: f32,
) -> EditMesh {
    if selected.is_empty() || inset_fraction.abs() < 1e-6 {
        return mesh.clone();
    }

    let frac = inset_fraction.clamp(0.001, 0.999);

    let mut new_positions = mesh.positions.clone();
    let mut new_normals = mesh.normals.clone();
    let mut new_uvs = mesh.uvs.clone();
    let mut new_triangles = Vec::with_capacity(mesh.triangles.len() + selected.len() * 6);

    // Copy non-selected triangles unchanged
    for (fi, tri) in mesh.triangles.iter().enumerate() {
        if !selected.contains(&fi) {
            new_triangles.push(*tri);
        }
    }

    // Process each selected face
    for &fi in selected {
        if fi >= mesh.triangles.len() {
            continue;
        }

        let [a, b, c] = mesh.triangles[fi];
        let pa = mesh.positions[a as usize];
        let pb = mesh.positions[b as usize];
        let pc = mesh.positions[c as usize];
        let center = (pa + pb + pc) / 3.0;

        let na = mesh.normals[a as usize];
        let nb = mesh.normals[b as usize];
        let nc = mesh.normals[c as usize];
        let ncenter = ((na + nb + nc) / 3.0).normalize_or_zero();

        let ua = mesh.uvs[a as usize];
        let ub = mesh.uvs[b as usize];
        let uc = mesh.uvs[c as usize];
        let uv_center = (ua + ub + uc) / 3.0;

        // Create inner vertices (lerped toward centroid)
        let ia = new_positions.len() as u32;
        new_positions.push(pa.lerp(center, frac));
        new_normals.push(na.lerp(ncenter, frac).normalize_or_zero());
        new_uvs.push(ua.lerp(uv_center, frac));

        let ib = new_positions.len() as u32;
        new_positions.push(pb.lerp(center, frac));
        new_normals.push(nb.lerp(ncenter, frac).normalize_or_zero());
        new_uvs.push(ub.lerp(uv_center, frac));

        let ic = new_positions.len() as u32;
        new_positions.push(pc.lerp(center, frac));
        new_normals.push(nc.lerp(ncenter, frac).normalize_or_zero());
        new_uvs.push(uc.lerp(uv_center, frac));

        // Inner triangle (same winding as original)
        new_triangles.push([ia, ib, ic]);

        // Bridge quads connecting outer edge to inner edge
        // Edge a→b: outer (a, b), inner (ia, ib)
        new_triangles.push([a, b, ib]);
        new_triangles.push([a, ib, ia]);

        // Edge b→c: outer (b, c), inner (ib, ic)
        new_triangles.push([b, c, ic]);
        new_triangles.push([b, ic, ib]);

        // Edge c→a: outer (c, a), inner (ic, ia)
        new_triangles.push([c, a, ia]);
        new_triangles.push([c, ia, ic]);
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
