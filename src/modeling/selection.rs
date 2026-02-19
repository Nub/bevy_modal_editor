//! Face picking and grid-based selection algorithms.
//!
//! Provides ray-triangle intersection for face picking, plus grid-based
//! selection modes (world-space, surface-space, UV-space, freeform polygon).

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use super::edit_mesh::{EditMesh, FaceIndex};
use super::half_edge::HalfEdgeMesh;

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

/// Select all faces whose world-space center falls in the same grid cell as `hit_face`,
/// filtered to only include faces whose normal is similar to `hit_face`.
///
/// Always includes the hit triangle itself, even if its center technically lands in
/// a neighboring cell (this can happen when clicking near a cell boundary).
pub fn world_grid_select(
    mesh: &EditMesh,
    transform: &GlobalTransform,
    grid_size: f32,
    _point: Vec3,
    hit_face: FaceIndex,
) -> HashSet<FaceIndex> {
    // Use the hit triangle's center to determine the grid cell — this guarantees
    // we always find at least the hit triangle in the grid lookup.
    let hit_center = transform.transform_point(mesh.face_center(hit_face));
    let cell = IVec3::new(
        (hit_center.x / grid_size).floor() as i32,
        (hit_center.y / grid_size).floor() as i32,
        (hit_center.z / grid_size).floor() as i32,
    );

    let hit_normal = mesh.face_normal(hit_face);

    let grid = build_world_grid(mesh, transform, grid_size);
    let mut result: HashSet<FaceIndex> = grid
        .get(&cell)
        .map(|faces| {
            faces
                .iter()
                .copied()
                .filter(|&fi| mesh.face_normal(fi).dot(hit_normal) >= COPLANAR_COS)
                .collect()
        })
        .unwrap_or_default();

    // Always include the hit triangle
    result.insert(hit_face);

    result
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

/// Select all faces whose average UV falls in the same UV grid cell as the hit face,
/// filtered to only include faces with a similar normal.
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

    let hit_normal = mesh.face_normal(hit_face);

    let grid = build_uv_grid(mesh, uv_grid_size);
    grid.get(&cell)
        .map(|faces| {
            faces
                .iter()
                .copied()
                .filter(|&fi| mesh.face_normal(fi).dot(hit_normal) >= COPLANAR_COS)
                .collect()
        })
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

// ---------------------------------------------------------------------------
// Coplanar Face Groups
// ---------------------------------------------------------------------------

/// Cosine threshold for considering two triangles coplanar (very tight — ~2.5°).
const COPLANAR_COS: f32 = 0.999;

/// Flood-fill from a start triangle to find all adjacent coplanar triangles.
///
/// Two adjacent triangles are grouped if their normals have a dot product >= COPLANAR_COS.
/// This identifies "logical faces" — e.g. the two triangles forming one side of a cube.
pub fn coplanar_flood_fill(mesh: &EditMesh, start_face: FaceIndex) -> HashSet<FaceIndex> {
    let adj = mesh.build_adjacency();
    let start_normal = mesh.face_normal(start_face);

    let mut group = HashSet::new();
    let mut frontier = vec![start_face];

    while let Some(fi) = frontier.pop() {
        if !group.insert(fi) {
            continue;
        }
        for edge in mesh.face_edges(fi) {
            if let Some(neighbors) = adj.get(&edge) {
                for &neighbor in neighbors {
                    if group.contains(&neighbor) {
                        continue;
                    }
                    let n = mesh.face_normal(neighbor);
                    if start_normal.dot(n) >= COPLANAR_COS {
                        frontier.push(neighbor);
                    }
                }
            }
        }
    }

    group
}

/// Expand a selection so every coplanar face group is either fully included or excluded.
///
/// For each selected triangle, flood-fills to find its complete coplanar group
/// and includes all of those triangles.
pub fn expand_to_face_groups(mesh: &EditMesh, selection: &HashSet<FaceIndex>) -> HashSet<FaceIndex> {
    let mut expanded = HashSet::new();
    for &fi in selection {
        if expanded.contains(&fi) {
            continue;
        }
        let group = coplanar_flood_fill(mesh, fi);
        expanded.extend(group);
    }
    expanded
}

// ---------------------------------------------------------------------------
// Vertex Picking (screen-space proximity)
// ---------------------------------------------------------------------------

/// Result of a vertex pick operation.
#[derive(Debug, Clone, Copy)]
pub struct VertexHit {
    pub vertex: u32,
    pub screen_distance: f32,
}

/// Pick the closest vertex to the cursor in screen space.
///
/// Projects all mesh vertices to screen space and returns the closest one
/// within `max_screen_distance` pixels.
pub fn pick_vertex(
    mesh: &EditMesh,
    mesh_transform: &GlobalTransform,
    cursor_pos: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    max_screen_distance: f32,
) -> Option<VertexHit> {
    let mut closest: Option<VertexHit> = None;

    for (vi, pos) in mesh.positions.iter().enumerate() {
        let world_pos = mesh_transform.transform_point(*pos);
        if let Ok(screen_pos) = camera.world_to_viewport(camera_transform, world_pos) {
            let dist = screen_pos.distance(cursor_pos);
            if dist <= max_screen_distance {
                if closest.is_none() || dist < closest.unwrap().screen_distance {
                    closest = Some(VertexHit {
                        vertex: vi as u32,
                        screen_distance: dist,
                    });
                }
            }
        }
    }

    closest
}

// ---------------------------------------------------------------------------
// Edge Picking (closest edge to ray)
// ---------------------------------------------------------------------------

/// Result of an edge pick operation.
#[derive(Debug, Clone, Copy)]
pub struct EdgeHit {
    /// Half-edge index (canonical — the lower of the pair with its twin).
    pub half_edge: u32,
    pub screen_distance: f32,
}

/// Pick the closest edge to the cursor in screen space.
///
/// Projects edge midpoints to screen space and returns the closest one
/// within `max_screen_distance` pixels. Returns the canonical half-edge index.
pub fn pick_edge(
    he_mesh: &HalfEdgeMesh,
    mesh_transform: &GlobalTransform,
    cursor_pos: Vec2,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    max_screen_distance: f32,
    xray: bool,
) -> Option<EdgeHit> {
    let mut closest: Option<EdgeHit> = None;

    for he_id in he_mesh.unique_edges() {
        let (from, to) = he_mesh.edge_vertices(he_id);
        let p0 = mesh_transform.transform_point(he_mesh.vertices[from as usize].position);
        let p1 = mesh_transform.transform_point(he_mesh.vertices[to as usize].position);

        // Skip back-facing edges unless xray is enabled
        if !xray {
            let face_id = he_mesh.half_edges[he_id as usize].face;
            if face_id != super::half_edge::INVALID {
                let face_normal = he_mesh.face_normal(face_id);
                let world_normal = mesh_transform
                    .affine()
                    .transform_vector3(face_normal)
                    .normalize_or_zero();
                let mid = (p0 + p1) * 0.5;
                let view_dir = (mid - camera_transform.translation()).normalize_or_zero();
                if world_normal.dot(view_dir) >= 0.0 {
                    // Check twin face too
                    let twin = he_mesh.half_edges[he_id as usize].twin;
                    if twin == super::half_edge::INVALID {
                        continue;
                    }
                    let twin_face = he_mesh.half_edges[twin as usize].face;
                    if twin_face == super::half_edge::INVALID {
                        continue;
                    }
                    let twin_normal = he_mesh.face_normal(twin_face);
                    let twin_world_normal = mesh_transform
                        .affine()
                        .transform_vector3(twin_normal)
                        .normalize_or_zero();
                    if twin_world_normal.dot(view_dir) >= 0.0 {
                        continue;
                    }
                }
            }
        }

        // Project edge endpoints to screen space and find closest point on segment
        let Ok(s0) = camera.world_to_viewport(camera_transform, p0) else {
            continue;
        };
        let Ok(s1) = camera.world_to_viewport(camera_transform, p1) else {
            continue;
        };

        let dist = point_to_segment_distance(cursor_pos, s0, s1);
        if dist <= max_screen_distance {
            if closest.is_none() || dist < closest.unwrap().screen_distance {
                closest = Some(EdgeHit {
                    half_edge: he_id,
                    screen_distance: dist,
                });
            }
        }
    }

    closest
}

/// Minimum distance from a point to a line segment in 2D.
fn point_to_segment_distance(point: Vec2, seg_a: Vec2, seg_b: Vec2) -> f32 {
    let ab = seg_b - seg_a;
    let ap = point - seg_a;
    let len_sq = ab.length_squared();
    if len_sq < 1e-10 {
        return ap.length();
    }
    let t = (ap.dot(ab) / len_sq).clamp(0.0, 1.0);
    let closest = seg_a + ab * t;
    point.distance(closest)
}
