//! Face picking and grid-based selection algorithms.
//!
//! Provides ray-triangle intersection for face picking, plus grid-based
//! selection modes (world-space, surface-space, UV-space, freeform polygon).

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::edit_mesh::{EditMesh, FaceIndex};

/// Result of a face pick operation.
#[derive(Debug, Clone, Copy)]
pub struct FaceHit {
    pub face: FaceIndex,
    pub point: Vec3,
    pub distance: f32,
}

/// Moller-Trumbore ray-triangle intersection.
///
/// Returns the distance along the ray if the ray hits the triangle.
fn ray_triangle_intersection(
    ray_origin: Vec3,
    ray_dir: Vec3,
    v0: Vec3,
    v1: Vec3,
    v2: Vec3,
) -> Option<f32> {
    const EPSILON: f32 = 1e-7;

    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let h = ray_dir.cross(edge2);
    let a = edge1.dot(h);

    if a.abs() < EPSILON {
        return None;
    }

    let f = 1.0 / a;
    let s = ray_origin - v0;
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }

    let q = s.cross(edge1);
    let v = f * ray_dir.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }

    let t = f * edge2.dot(q);
    if t > EPSILON {
        Some(t)
    } else {
        None
    }
}

/// Pick the closest face hit by a ray against the mesh (in local space).
///
/// `ray_origin` and `ray_dir` should be in the mesh's local coordinate space.
/// When `xray` is false, only front-facing triangles (normal facing the camera) are picked.
pub fn pick_face(mesh: &EditMesh, ray_origin: Vec3, ray_dir: Vec3, xray: bool) -> Option<FaceHit> {
    let mut closest: Option<FaceHit> = None;

    for (fi, tri) in mesh.triangles.iter().enumerate() {
        let v0 = mesh.positions[tri[0] as usize];
        let v1 = mesh.positions[tri[1] as usize];
        let v2 = mesh.positions[tri[2] as usize];

        // Skip back-facing triangles unless xray is enabled
        if !xray {
            let face_normal = (v1 - v0).cross(v2 - v0);
            if face_normal.dot(ray_dir) >= 0.0 {
                continue;
            }
        }

        if let Some(t) = ray_triangle_intersection(ray_origin, ray_dir, v0, v1, v2) {
            if closest.is_none() || t < closest.unwrap().distance {
                closest = Some(FaceHit {
                    face: fi,
                    point: ray_origin + ray_dir * t,
                    distance: t,
                });
            }
        }
    }

    closest
}

/// Transform a world-space ray into the mesh's local space.
pub fn world_to_local_ray(
    transform: &GlobalTransform,
    ray_origin: Vec3,
    ray_dir: Vec3,
) -> (Vec3, Vec3) {
    let inv = transform.affine().inverse();
    let local_origin = inv.transform_point3(ray_origin);
    let local_target = inv.transform_point3(ray_origin + ray_dir);
    let local_dir = (local_target - local_origin).normalize();
    (local_origin, local_dir)
}

// ---------------------------------------------------------------------------
// World-Space Grid Selection
// ---------------------------------------------------------------------------

/// Build a mapping from world-space grid cells to faces.
pub fn build_world_grid(
    mesh: &EditMesh,
    transform: &GlobalTransform,
    grid_size: f32,
) -> HashMap<IVec3, Vec<FaceIndex>> {
    let mut grid: HashMap<IVec3, Vec<FaceIndex>> = HashMap::new();

    for fi in 0..mesh.triangles.len() {
        let local_center = mesh.face_center(fi);
        let world_center = transform.transform_point(local_center);
        let cell = IVec3::new(
            (world_center.x / grid_size).floor() as i32,
            (world_center.y / grid_size).floor() as i32,
            (world_center.z / grid_size).floor() as i32,
        );
        grid.entry(cell).or_default().push(fi);
    }

    grid
}

/// Select all faces whose world-space center falls in the same grid cell as `point`.
pub fn world_grid_select(
    mesh: &EditMesh,
    transform: &GlobalTransform,
    grid_size: f32,
    point: Vec3,
) -> HashSet<FaceIndex> {
    let cell = IVec3::new(
        (point.x / grid_size).floor() as i32,
        (point.y / grid_size).floor() as i32,
        (point.z / grid_size).floor() as i32,
    );

    let grid = build_world_grid(mesh, transform, grid_size);
    grid.get(&cell)
        .map(|faces| faces.iter().copied().collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Surface-Space Grid Selection (flood-fill by normal similarity)
// ---------------------------------------------------------------------------

/// Select a contiguous region of faces with similar normals, starting from `start_face`.
pub fn surface_group_select(
    mesh: &EditMesh,
    start_face: FaceIndex,
    angle_threshold_degrees: f32,
) -> HashSet<FaceIndex> {
    let adj = mesh.build_adjacency();
    let threshold_cos = angle_threshold_degrees.to_radians().cos();
    let start_normal = mesh.face_normal(start_face);

    let mut selected = HashSet::new();
    let mut frontier = vec![start_face];

    while let Some(fi) = frontier.pop() {
        if !selected.insert(fi) {
            continue;
        }

        // Check neighbors via shared edges
        for edge in mesh.face_edges(fi) {
            if let Some(neighbors) = adj.get(&edge) {
                for &neighbor in neighbors {
                    if selected.contains(&neighbor) {
                        continue;
                    }
                    let neighbor_normal = mesh.face_normal(neighbor);
                    if start_normal.dot(neighbor_normal) >= threshold_cos {
                        frontier.push(neighbor);
                    }
                }
            }
        }
    }

    selected
}

// ---------------------------------------------------------------------------
// UV-Space Grid Selection
// ---------------------------------------------------------------------------

/// Build a mapping from UV grid cells to faces.
pub fn build_uv_grid(
    mesh: &EditMesh,
    uv_grid_size: f32,
) -> HashMap<IVec2, Vec<FaceIndex>> {
    let mut grid: HashMap<IVec2, Vec<FaceIndex>> = HashMap::new();

    for fi in 0..mesh.triangles.len() {
        let avg_uv = mesh.face_uv_center(fi);
        let cell = IVec2::new(
            (avg_uv.x / uv_grid_size).floor() as i32,
            (avg_uv.y / uv_grid_size).floor() as i32,
        );
        grid.entry(cell).or_default().push(fi);
    }

    grid
}

/// Select all faces whose average UV falls in the same UV grid cell as the hit face.
pub fn uv_grid_select(
    mesh: &EditMesh,
    hit_face: FaceIndex,
    uv_grid_size: f32,
) -> HashSet<FaceIndex> {
    let avg_uv = mesh.face_uv_center(hit_face);
    let cell = IVec2::new(
        (avg_uv.x / uv_grid_size).floor() as i32,
        (avg_uv.y / uv_grid_size).floor() as i32,
    );

    let grid = build_uv_grid(mesh, uv_grid_size);
    grid.get(&cell)
        .map(|faces| faces.iter().copied().collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Freeform Polygon Selection
// ---------------------------------------------------------------------------

/// Select faces whose projected center (in screen space) falls inside a 2D polygon.
///
/// `polygon_screen` is a closed polygon in screen coordinates.
/// `camera` and `camera_transform` are used to project face centers to screen space.
pub fn freeform_select(
    mesh: &EditMesh,
    mesh_transform: &GlobalTransform,
    polygon_screen: &[Vec2],
    camera: &Camera,
    camera_transform: &GlobalTransform,
) -> HashSet<FaceIndex> {
    if polygon_screen.len() < 3 {
        return HashSet::new();
    }

    let mut selected = HashSet::new();

    for fi in 0..mesh.triangles.len() {
        let local_center = mesh.face_center(fi);
        let world_center = mesh_transform.transform_point(local_center);

        // Project to screen space
        if let Ok(screen_pos) = camera.world_to_viewport(camera_transform, world_center) {
            if point_in_polygon(screen_pos, polygon_screen) {
                selected.insert(fi);
            }
        }
    }

    selected
}

/// Winding-number test for point-in-polygon (2D).
fn point_in_polygon(point: Vec2, polygon: &[Vec2]) -> bool {
    let mut winding = 0i32;
    let n = polygon.len();

    for i in 0..n {
        let v0 = polygon[i];
        let v1 = polygon[(i + 1) % n];

        if v0.y <= point.y {
            if v1.y > point.y {
                // Upward crossing
                if cross_2d(v1 - v0, point - v0) > 0.0 {
                    winding += 1;
                }
            }
        } else if v1.y <= point.y {
            // Downward crossing
            if cross_2d(v1 - v0, point - v0) < 0.0 {
                winding -= 1;
            }
        }
    }

    winding != 0
}

/// 2D cross product (z-component of 3D cross).
fn cross_2d(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}
