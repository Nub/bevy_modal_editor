//! UV projection algorithms.
//!
//! Provides box, planar, and cylindrical UV projection for mesh faces.

use bevy::prelude::*;
use std::collections::HashSet;

use super::edit_mesh::{EditMesh, FaceIndex};

/// UV projection method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvProjection {
    /// 6-direction planar: each face assigned to axis most aligned with its normal.
    Box,
    /// Project all UVs onto a single plane.
    Planar,
    /// Cylindrical unwrap around an axis.
    Cylindrical,
}

impl UvProjection {
    pub fn display_name(&self) -> &'static str {
        match self {
            UvProjection::Box => "Box",
            UvProjection::Planar => "Planar",
            UvProjection::Cylindrical => "Cylindrical",
        }
    }
}

/// Axis for planar/cylindrical projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionAxis {
    X,
    Y,
    Z,
}

impl ProjectionAxis {
    pub fn display_name(&self) -> &'static str {
        match self {
            ProjectionAxis::X => "X",
            ProjectionAxis::Y => "Y",
            ProjectionAxis::Z => "Z",
        }
    }
}

/// Apply UV projection to the entire mesh.
pub fn project_uvs(
    mesh: &EditMesh,
    method: UvProjection,
    axis: ProjectionAxis,
    scale: f32,
) -> EditMesh {
    let faces: HashSet<FaceIndex> = (0..mesh.triangles.len()).collect();
    project_uvs_faces(mesh, &faces, method, axis, scale)
}

/// Apply UV projection to selected faces only.
pub fn project_uvs_faces(
    mesh: &EditMesh,
    faces: &HashSet<FaceIndex>,
    method: UvProjection,
    axis: ProjectionAxis,
    scale: f32,
) -> EditMesh {
    let mut result = mesh.clone();
    let scale = if scale.abs() < 1e-6 { 1.0 } else { scale };

    match method {
        UvProjection::Box => box_project(&mut result, faces, scale),
        UvProjection::Planar => planar_project(&mut result, faces, axis, scale),
        UvProjection::Cylindrical => cylindrical_project(&mut result, faces, axis, scale),
    }

    result
}

/// Box projection: assign each face to the axis most aligned with its normal,
/// then project UVs onto that axis-aligned plane.
fn box_project(mesh: &mut EditMesh, faces: &HashSet<FaceIndex>, scale: f32) {
    // Collect all vertices used by selected faces
    let mut affected_verts: HashSet<u32> = HashSet::new();
    for &fi in faces {
        if fi < mesh.triangles.len() {
            for &v in &mesh.triangles[fi] {
                affected_verts.insert(v);
            }
        }
    }

    // For each affected vertex, determine projection from dominant face normal
    // We use per-face projection to handle vertices shared between differently-oriented faces
    // by processing per-triangle rather than per-vertex.
    // Since EditMesh may share vertices between faces with different projections,
    // we process each face and set UVs for its vertices.
    // Note: this may set conflicting UVs for shared vertices — box projection
    // generally requires per-face-corner UVs (which our flat EditMesh supports
    // if vertices are split per face).

    for &fi in faces {
        if fi >= mesh.triangles.len() {
            continue;
        }
        let normal = mesh.face_normal(fi);
        let abs_n = normal.abs();

        // Determine dominant axis
        let (u_fn, v_fn): (fn(Vec3) -> f32, fn(Vec3) -> f32) = if abs_n.x >= abs_n.y && abs_n.x >= abs_n.z {
            // X-dominant: project onto YZ
            (|p: Vec3| p.y, |p: Vec3| p.z)
        } else if abs_n.y >= abs_n.x && abs_n.y >= abs_n.z {
            // Y-dominant: project onto XZ
            (|p: Vec3| p.x, |p: Vec3| p.z)
        } else {
            // Z-dominant: project onto XY
            (|p: Vec3| p.x, |p: Vec3| p.y)
        };

        for &v in &mesh.triangles[fi] {
            let pos = mesh.positions[v as usize];
            mesh.uvs[v as usize] = Vec2::new(u_fn(pos) * scale, v_fn(pos) * scale);
        }
    }
}

/// Planar projection: project all UVs onto a plane perpendicular to the chosen axis.
fn planar_project(mesh: &mut EditMesh, faces: &HashSet<FaceIndex>, axis: ProjectionAxis, scale: f32) {
    let (u_fn, v_fn): (fn(Vec3) -> f32, fn(Vec3) -> f32) = match axis {
        ProjectionAxis::X => (|p: Vec3| p.y, |p: Vec3| p.z),
        ProjectionAxis::Y => (|p: Vec3| p.x, |p: Vec3| p.z),
        ProjectionAxis::Z => (|p: Vec3| p.x, |p: Vec3| p.y),
    };

    for &fi in faces {
        if fi >= mesh.triangles.len() {
            continue;
        }
        for &v in &mesh.triangles[fi] {
            let pos = mesh.positions[v as usize];
            mesh.uvs[v as usize] = Vec2::new(u_fn(pos) * scale, v_fn(pos) * scale);
        }
    }
}

/// Cylindrical projection: unwrap around the chosen axis.
fn cylindrical_project(mesh: &mut EditMesh, faces: &HashSet<FaceIndex>, axis: ProjectionAxis, scale: f32) {
    for &fi in faces {
        if fi >= mesh.triangles.len() {
            continue;
        }
        for &v in &mesh.triangles[fi] {
            let pos = mesh.positions[v as usize];
            let (angle, height) = match axis {
                ProjectionAxis::X => (pos.z.atan2(pos.y), pos.x),
                ProjectionAxis::Y => (pos.x.atan2(pos.z), pos.y),
                ProjectionAxis::Z => (pos.y.atan2(pos.x), pos.z),
            };
            // Normalize angle from [-PI, PI] to [0, 1]
            let u = (angle / std::f32::consts::TAU) + 0.5;
            mesh.uvs[v as usize] = Vec2::new(u * scale, height * scale);
        }
    }
}
