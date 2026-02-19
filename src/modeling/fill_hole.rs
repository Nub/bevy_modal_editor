//! Hole filling for meshes with open boundaries.
//!
//! Detects boundary edge loops (edges with only one adjacent face)
//! and fills them with triangles using ear-clipping triangulation.

use bevy::prelude::*;
use std::collections::HashMap;

use super::edit_mesh::EditMesh;

/// Fill all holes in the mesh by triangulating boundary loops.
///
/// A "hole" is a loop of boundary edges (edges with only one adjacent face).
/// Each hole is filled by ear-clipping triangulation projected onto
/// the loop's average plane.
pub fn fill_holes(mesh: &EditMesh) -> EditMesh {
    let mut result = mesh.clone();
    let loops = find_boundary_loops(mesh);

    for boundary_loop in &loops {
        if boundary_loop.len() < 3 {
            continue;
        }
        let new_tris = triangulate_loop(boundary_loop, &result.positions);
        result.triangles.extend(new_tris);
    }

    result.recompute_normals();
    result
}

/// Find all boundary edge loops in the mesh.
///
/// Returns a list of vertex index loops, each representing one hole boundary.
fn find_boundary_loops(mesh: &EditMesh) -> Vec<Vec<u32>> {
    let adj = mesh.build_adjacency();

    // Collect boundary edges (only one adjacent face)
    let mut boundary_edges: HashMap<u32, Vec<u32>> = HashMap::new();
    for (edge, faces) in &adj {
        if faces.len() == 1 {
            boundary_edges.entry(edge.0).or_default().push(edge.1);
            boundary_edges.entry(edge.1).or_default().push(edge.0);
        }
    }

    // Trace loops
    let mut visited = std::collections::HashSet::new();
    let mut loops = Vec::new();

    for &start in boundary_edges.keys() {
        if visited.contains(&start) {
            continue;
        }

        let mut current_loop = Vec::new();
        let mut current = start;
        let mut prev = u32::MAX;

        loop {
            if visited.contains(&current) && current != start {
                break;
            }
            visited.insert(current);
            current_loop.push(current);

            let Some(neighbors) = boundary_edges.get(&current) else {
                break;
            };

            // Pick the next unvisited neighbor (or close the loop)
            let next = neighbors
                .iter()
                .copied()
                .find(|&n| n != prev && (!visited.contains(&n) || n == start));

            match next {
                Some(n) if n == start && current_loop.len() >= 3 => {
                    // Loop closed
                    break;
                }
                Some(n) => {
                    prev = current;
                    current = n;
                }
                None => break, // Dead end (shouldn't happen for valid boundary)
            }
        }

        if current_loop.len() >= 3 {
            loops.push(current_loop);
        }
    }

    loops
}

/// Triangulate a vertex loop using ear-clipping.
///
/// Projects the loop onto its average plane for 2D ear detection,
/// then creates triangles in 3D.
fn triangulate_loop(vertex_loop: &[u32], positions: &[Vec3]) -> Vec<[u32; 3]> {
    if vertex_loop.len() < 3 {
        return Vec::new();
    }
    if vertex_loop.len() == 3 {
        return vec![[vertex_loop[0], vertex_loop[1], vertex_loop[2]]];
    }

    // Compute loop normal (area-weighted)
    let center: Vec3 = vertex_loop
        .iter()
        .map(|&vi| positions[vi as usize])
        .sum::<Vec3>()
        / vertex_loop.len() as f32;

    let mut normal = Vec3::ZERO;
    for i in 0..vertex_loop.len() {
        let j = (i + 1) % vertex_loop.len();
        let a = positions[vertex_loop[i] as usize] - center;
        let b = positions[vertex_loop[j] as usize] - center;
        normal += a.cross(b);
    }
    normal = normal.normalize_or_zero();
    if normal == Vec3::ZERO {
        normal = Vec3::Y; // Fallback
    }

    // Project to 2D for ear detection
    let (u_axis, v_axis) = make_orthonormal_basis(normal);
    let project = |vi: u32| -> Vec2 {
        let p = positions[vi as usize] - center;
        Vec2::new(p.dot(u_axis), p.dot(v_axis))
    };

    // Ear-clip
    let mut remaining: Vec<u32> = vertex_loop.to_vec();
    let mut triangles = Vec::new();

    let mut safety = remaining.len() * remaining.len();
    while remaining.len() > 3 && safety > 0 {
        safety -= 1;
        let n = remaining.len();
        let mut found_ear = false;

        for i in 0..n {
            let prev = (i + n - 1) % n;
            let next = (i + 1) % n;

            let a = project(remaining[prev]);
            let b = project(remaining[i]);
            let c = project(remaining[next]);

            // Check if this is a convex vertex (ear tip candidate)
            if cross_2d(b - a, c - a) <= 0.0 {
                continue; // Reflex vertex
            }

            // Check no other vertex is inside this triangle
            let has_interior_point = (0..n).any(|k| {
                k != prev
                    && k != i
                    && k != next
                    && point_in_triangle_2d(project(remaining[k]), a, b, c)
            });

            if has_interior_point {
                continue;
            }

            // Valid ear — clip it
            triangles.push([remaining[prev], remaining[i], remaining[next]]);
            remaining.remove(i);
            found_ear = true;
            break;
        }

        if !found_ear {
            break; // Degenerate polygon
        }
    }

    // Last triangle
    if remaining.len() == 3 {
        triangles.push([remaining[0], remaining[1], remaining[2]]);
    }

    triangles
}

/// 2D cross product (z-component).
fn cross_2d(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// Check if point P is inside triangle ABC (2D, using barycentric coordinates).
fn point_in_triangle_2d(p: Vec2, a: Vec2, b: Vec2, c: Vec2) -> bool {
    let v0 = c - a;
    let v1 = b - a;
    let v2 = p - a;

    let dot00 = v0.dot(v0);
    let dot01 = v0.dot(v1);
    let dot02 = v0.dot(v2);
    let dot11 = v1.dot(v1);
    let dot12 = v1.dot(v2);

    let inv_denom = 1.0 / (dot00 * dot11 - dot01 * dot01);
    let u = (dot11 * dot02 - dot01 * dot12) * inv_denom;
    let v = (dot00 * dot12 - dot01 * dot02) * inv_denom;

    u >= 0.0 && v >= 0.0 && u + v < 1.0
}

/// Build an orthonormal basis from a normal vector.
fn make_orthonormal_basis(normal: Vec3) -> (Vec3, Vec3) {
    let up = if normal.y.abs() < 0.99 {
        Vec3::Y
    } else {
        Vec3::X
    };
    let u = normal.cross(up).normalize();
    let v = normal.cross(u).normalize();
    (u, v)
}
