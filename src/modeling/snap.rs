//! Vertex and edge snapping utilities.
//!
//! Provides grid-based vertex snapping and snap-to-nearest-vertex/edge for
//! precise mesh editing.

use bevy::prelude::*;
use std::collections::HashSet;

use super::edit_mesh::{EditMesh, FaceIndex};

/// Snap mode for vertex transforms.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapMode {
    /// No snapping.
    #[default]
    None,
    /// Snap to world grid.
    Grid,
    /// Snap to nearest vertex.
    Vertex,
    /// Snap to edge midpoints.
    EdgeMidpoint,
}

impl SnapMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            SnapMode::None => "None",
            SnapMode::Grid => "Grid",
            SnapMode::Vertex => "Vertex",
            SnapMode::EdgeMidpoint => "Edge Mid",
        }
    }
}

/// Snap a position to the world grid.
pub fn snap_to_grid(pos: Vec3, grid_size: f32) -> Vec3 {
    Vec3::new(
        (pos.x / grid_size).round() * grid_size,
        (pos.y / grid_size).round() * grid_size,
        (pos.z / grid_size).round() * grid_size,
    )
}

/// Snap a position to the nearest vertex in the mesh (excluding vertices in `exclude`).
pub fn snap_to_vertex(pos: Vec3, mesh: &EditMesh, exclude: &HashSet<u32>, max_dist: f32) -> Option<Vec3> {
    let mut closest: Option<(Vec3, f32)> = None;

    for (vi, &vert_pos) in mesh.positions.iter().enumerate() {
        if exclude.contains(&(vi as u32)) {
            continue;
        }
        let dist = pos.distance(vert_pos);
        if dist <= max_dist {
            if closest.is_none() || dist < closest.unwrap().1 {
                closest = Some((vert_pos, dist));
            }
        }
    }

    closest.map(|(p, _)| p)
}

/// Snap a position to the nearest edge midpoint.
pub fn snap_to_edge_midpoint(pos: Vec3, mesh: &EditMesh, max_dist: f32) -> Option<Vec3> {
    let mut closest: Option<(Vec3, f32)> = None;

    let mut seen_edges = HashSet::new();
    for tri in &mesh.triangles {
        for i in 0..3 {
            let a = tri[i].min(tri[(i + 1) % 3]);
            let b = tri[i].max(tri[(i + 1) % 3]);
            if !seen_edges.insert((a, b)) {
                continue;
            }
            let mid = (mesh.positions[a as usize] + mesh.positions[b as usize]) * 0.5;
            let dist = pos.distance(mid);
            if dist <= max_dist {
                if closest.is_none() || dist < closest.unwrap().1 {
                    closest = Some((mid, dist));
                }
            }
        }
    }

    closest.map(|(p, _)| p)
}

/// Apply snapping to a proposed vertex position based on the active snap mode.
pub fn apply_snap(
    pos: Vec3,
    mode: SnapMode,
    grid_size: f32,
    mesh: &EditMesh,
    exclude: &HashSet<u32>,
) -> Vec3 {
    match mode {
        SnapMode::None => pos,
        SnapMode::Grid => snap_to_grid(pos, grid_size),
        SnapMode::Vertex => snap_to_vertex(pos, mesh, exclude, grid_size * 2.0).unwrap_or(pos),
        SnapMode::EdgeMidpoint => snap_to_edge_midpoint(pos, mesh, grid_size * 2.0).unwrap_or(pos),
    }
}

/// Snap selected vertices to the grid.
pub fn snap_vertices_to_grid(
    mesh: &EditMesh,
    selected: &HashSet<u32>,
    grid_size: f32,
) -> EditMesh {
    let mut result = mesh.clone();
    for &vi in selected {
        if (vi as usize) < result.positions.len() {
            result.positions[vi as usize] = snap_to_grid(result.positions[vi as usize], grid_size);
        }
    }
    result.recompute_normals();
    result
}

/// Snap selected faces' vertices to the grid.
pub fn snap_faces_to_grid(
    mesh: &EditMesh,
    selected: &HashSet<FaceIndex>,
    grid_size: f32,
) -> EditMesh {
    let verts = mesh.selected_vertices(selected);
    snap_vertices_to_grid(mesh, &verts, grid_size)
}
