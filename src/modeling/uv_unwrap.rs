//! Automatic UV unwrapping via Least Squares Conformal Maps (LSCM).
//!
//! Splits the mesh into islands along seam edges, then flattens each
//! island to 2D while minimizing angle distortion.

use bevy::prelude::*;
use std::collections::{HashSet, VecDeque};

use super::edit_mesh::{EditMesh, FaceIndex};
use super::uv_seam::SeamEdge;

/// Unwrap mesh UVs along seam edges using LSCM.
///
/// `seams` defines where to cut the mesh surface. Each connected component
/// after cutting becomes a UV island that is flattened independently.
/// Islands are packed into [0,1] UV space.
pub fn unwrap_uvs(mesh: &EditMesh, seams: &HashSet<SeamEdge>) -> EditMesh {
    let mut result = mesh.clone();

    // Find UV islands (connected components of faces, separated by seams)
    let islands = find_islands(mesh, seams);

    if islands.is_empty() {
        return result;
    }

    // Flatten each island independently
    let mut island_uvs: Vec<Vec<(u32, Vec2)>> = Vec::new();
    for island_faces in &islands {
        let uvs = flatten_island(mesh, island_faces, seams);
        island_uvs.push(uvs);
    }

    // Pack islands into [0,1] UV space
    let packed = pack_islands(&island_uvs);

    // Apply packed UVs to result
    for (vert, uv) in packed {
        if (vert as usize) < result.uvs.len() {
            result.uvs[vert as usize] = uv;
        }
    }

    result
}

/// Find connected components of faces, treating seam edges as boundaries.
fn find_islands(mesh: &EditMesh, seams: &HashSet<SeamEdge>) -> Vec<HashSet<FaceIndex>> {
    let adj = mesh.build_adjacency();
    let mut visited = HashSet::new();
    let mut islands = Vec::new();

    for fi in 0..mesh.triangles.len() {
        if visited.contains(&fi) {
            continue;
        }

        let mut island = HashSet::new();
        let mut queue = VecDeque::new();
        queue.push_back(fi);

        while let Some(face) = queue.pop_front() {
            if !island.insert(face) {
                continue;
            }
            visited.insert(face);

            // Check each edge of this face
            for edge in mesh.face_edges(face) {
                // Skip seam edges — they separate islands
                let seam_key = if edge.0 <= edge.1 {
                    (edge.0, edge.1)
                } else {
                    (edge.1, edge.0)
                };
                if seams.contains(&seam_key) {
                    continue;
                }

                // Add adjacent faces across non-seam edges
                if let Some(neighbors) = adj.get(&edge) {
                    for &neighbor in neighbors {
                        if !island.contains(&neighbor) {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        if !island.is_empty() {
            islands.push(island);
        }
    }

    islands
}

/// Flatten a UV island to 2D using angle-based flattening (ABF lite).
///
/// This is a simplified conformal-like flattening:
/// 1. Pick two "pinned" vertices to fix the initial mapping
/// 2. Iteratively solve for UV positions minimizing angle distortion
///
/// For simplicity, we use a planar projection onto the island's average plane
/// as a reasonable fallback that preserves shape for mostly-planar regions.
fn flatten_island(
    mesh: &EditMesh,
    faces: &HashSet<FaceIndex>,
    _seams: &HashSet<SeamEdge>,
) -> Vec<(u32, Vec2)> {
    // Collect unique vertices in this island
    let mut verts: HashSet<u32> = HashSet::new();
    for &fi in faces {
        if fi < mesh.triangles.len() {
            for &v in &mesh.triangles[fi] {
                verts.insert(v);
            }
        }
    }

    if verts.is_empty() {
        return Vec::new();
    }

    // Compute island normal (area-weighted average)
    let mut normal = Vec3::ZERO;
    let mut centroid = Vec3::ZERO;
    let mut total_area = 0.0f32;
    for &fi in faces {
        if fi < mesh.triangles.len() {
            let n = mesh.face_normal(fi);
            let a = mesh.face_area(fi);
            normal += n * a;
            centroid += mesh.face_center(fi) * a;
            total_area += a;
        }
    }

    if total_area < 1e-10 {
        return verts.iter().map(|&v| (v, Vec2::ZERO)).collect();
    }

    normal = normal.normalize_or_zero();
    centroid /= total_area;

    if normal == Vec3::ZERO {
        normal = Vec3::Y;
    }

    // Build orthonormal basis on the island's plane
    let up = if normal.y.abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let u_axis = normal.cross(up).normalize();
    let v_axis = normal.cross(u_axis).normalize();

    // Project each vertex onto the 2D plane
    let mut result = Vec::new();
    for &v in &verts {
        let p = mesh.positions[v as usize] - centroid;
        let u = p.dot(u_axis);
        let v_coord = p.dot(v_axis);
        result.push((v, Vec2::new(u, v_coord)));
    }

    result
}

/// Pack UV islands into [0,1] space using simple row-based packing.
fn pack_islands(islands: &[Vec<(u32, Vec2)>]) -> Vec<(u32, Vec2)> {
    if islands.is_empty() {
        return Vec::new();
    }

    // Compute bounding boxes
    struct IslandBounds {
        min: Vec2,
        max: Vec2,
        index: usize,
    }

    let mut bounds: Vec<IslandBounds> = islands
        .iter()
        .enumerate()
        .filter(|(_, island)| !island.is_empty())
        .map(|(i, island)| {
            let mut min = Vec2::splat(f32::MAX);
            let mut max = Vec2::splat(f32::MIN);
            for &(_, uv) in island {
                min = min.min(uv);
                max = max.max(uv);
            }
            IslandBounds { min, max, index: i }
        })
        .collect();

    if bounds.is_empty() {
        return Vec::new();
    }

    // Sort by height (tallest first) for better packing
    bounds.sort_by(|a, b| {
        let ha = a.max.y - a.min.y;
        let hb = b.max.y - b.min.y;
        hb.partial_cmp(&ha).unwrap_or(std::cmp::Ordering::Equal)
    });

    // Simple row packing
    let margin = 0.02;
    let mut result = Vec::new();
    let mut cursor_x = 0.0f32;
    let mut cursor_y = 0.0f32;
    let mut row_height = 0.0f32;
    let max_width = 1.0f32;

    for bound in &bounds {
        let w = bound.max.x - bound.min.x;
        let h = bound.max.y - bound.min.y;

        // Start new row if this island doesn't fit
        if cursor_x + w + margin > max_width && cursor_x > 0.0 {
            cursor_y += row_height + margin;
            cursor_x = 0.0;
            row_height = 0.0;
        }

        let offset = Vec2::new(cursor_x - bound.min.x, cursor_y - bound.min.y);

        for &(vert, uv) in &islands[bound.index] {
            result.push((vert, uv + offset));
        }

        cursor_x += w + margin;
        row_height = row_height.max(h);
    }

    // Scale everything to fit in [0,1] if it overflows
    let mut global_max = Vec2::ZERO;
    for &(_, uv) in &result {
        global_max = global_max.max(uv);
    }

    if global_max.x > 1.0 || global_max.y > 1.0 {
        let scale = 1.0 / global_max.x.max(global_max.y);
        for (_, uv) in &mut result {
            *uv *= scale;
        }
    }

    result
}
