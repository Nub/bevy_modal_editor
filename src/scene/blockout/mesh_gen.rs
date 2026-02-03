//! Procedural mesh generation for blockout shapes

use avian3d::prelude::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

use super::{ArchMarker, LShapeMarker, RampMarker, StairsMarker};

/// Helper to add a quad. Vertices p0->p1->p2->p3 should produce the desired normal via cross product.
/// The cross product (p1-p0) × (p2-p0) determines the face normal direction.
fn add_quad(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    p0: [f32; 3],
    p1: [f32; 3],
    p2: [f32; 3],
    p3: [f32; 3],
    normal: [f32; 3],
) {
    let base = positions.len() as u32;
    positions.extend_from_slice(&[p0, p1, p2, p3]);
    normals.extend_from_slice(&[normal; 4]);
    uvs.extend_from_slice(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    // Triangles: 0-1-2 and 0-2-3
    indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Helper to add a triangle. Vertices p0->p1->p2 should produce the desired normal via cross product.
fn add_triangle(
    positions: &mut Vec<[f32; 3]>,
    normals: &mut Vec<[f32; 3]>,
    uvs: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
    p0: [f32; 3],
    p1: [f32; 3],
    p2: [f32; 3],
    normal: [f32; 3],
) {
    let base = positions.len() as u32;
    positions.extend_from_slice(&[p0, p1, p2]);
    normals.extend_from_slice(&[normal; 3]);
    uvs.extend_from_slice(&[[0.0, 0.0], [1.0, 0.0], [0.5, 1.0]]);
    indices.extend_from_slice(&[base, base + 1, base + 2]);
}

/// Generate mesh for stairs
/// Stairs go from front (z=0) toward back (z=-depth), rising from y=0 to y=height
pub fn generate_stairs_mesh(params: &StairsMarker) -> Mesh {
    let step_height = params.height / params.step_count as f32;
    let step_depth = params.depth / params.step_count as f32;
    let hw = params.width / 2.0;

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // For each step:
    // z0 = front edge of this step (closer to 0)
    // z1 = back edge of this step (more negative)
    // y0 = bottom of riser
    // y1 = top of tread
    for i in 0..params.step_count {
        let y0 = i as f32 * step_height;
        let y1 = (i + 1) as f32 * step_height;
        let z0 = -(i as f32 * step_depth);
        let z1 = -((i + 1) as f32 * step_depth);

        // Top face (+Y normal)
        // Cross product (p1-p0) × (p2-p0) should give +Y
        // Using: front-right → back-right → back-left gives +Y
        add_quad(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            [hw, y1, z0],  // front-right
            [hw, y1, z1],  // back-right
            [-hw, y1, z1], // back-left
            [-hw, y1, z0], // front-left
            [0.0, 1.0, 0.0],
        );

        // Front face / riser (+Z normal)
        // Cross product should give +Z
        // Using: bottom-right → top-right → top-left gives +Z
        add_quad(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            [hw, y0, z0],  // bottom-right
            [hw, y1, z0],  // top-right
            [-hw, y1, z0], // top-left
            [-hw, y0, z0], // bottom-left
            [0.0, 0.0, 1.0],
        );

        // Left side (-X normal)
        // Cross product should give -X
        // Using: front-bottom → front-top → back-top gives -X
        add_quad(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            [-hw, y0, z0], // front-bottom
            [-hw, y1, z0], // front-top
            [-hw, y1, z1], // back-top
            [-hw, y0, z1], // back-bottom
            [-1.0, 0.0, 0.0],
        );

        // Right side (+X normal)
        // Cross product should give +X
        // Using: back-bottom → back-top → front-top gives +X
        add_quad(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            [hw, y0, z1], // back-bottom
            [hw, y1, z1], // back-top
            [hw, y1, z0], // front-top
            [hw, y0, z0], // front-bottom
            [1.0, 0.0, 0.0],
        );
    }

    // Back face (-Z normal) - full height back wall
    let z_back = -params.depth;
    // Cross product should give -Z
    // Using: bottom-right(-X) → top-right(-X) → top-left(+X) gives -Z
    add_quad(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        [-hw, 0.0, z_back],           // bottom-right (from back view)
        [-hw, params.height, z_back], // top-right
        [hw, params.height, z_back],  // top-left
        [hw, 0.0, z_back],            // bottom-left
        [0.0, 0.0, -1.0],
    );

    // Bottom face (-Y normal)
    // Cross product should give -Y
    // Using: front-left → back-left → back-right gives -Y
    add_quad(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        [-hw, 0.0, 0.0],    // front-left
        [-hw, 0.0, z_back], // back-left
        [hw, 0.0, z_back],  // back-right
        [hw, 0.0, 0.0],     // front-right
        [0.0, -1.0, 0.0],
    );

    // Side quads to fill the stepped profile below each step
    // For step i, fill from ground (y=0) to the step's bottom edge (y=i*step_height)
    // This connects ground level to where each step's side quad begins
    for i in 1..params.step_count {
        let y_top = i as f32 * step_height; // Bottom edge of step i's side quad
        let z0 = -(i as f32 * step_depth);  // Front of step i
        let z1 = -((i + 1) as f32 * step_depth); // Back of step i

        // Left side quad (-X normal)
        // Fills from ground to the bottom of this step's side
        add_quad(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            [-hw, 0.0, z0],   // front-bottom (ground)
            [-hw, y_top, z0], // front-top (step bottom edge)
            [-hw, y_top, z1], // back-top (step bottom edge)
            [-hw, 0.0, z1],   // back-bottom (ground)
            [-1.0, 0.0, 0.0],
        );

        // Right side quad (+X normal)
        // Fills from ground to the bottom of this step's side
        add_quad(
            &mut positions,
            &mut normals,
            &mut uvs,
            &mut indices,
            [hw, 0.0, z1],   // back-bottom (ground)
            [hw, y_top, z1], // back-top (step bottom edge)
            [hw, y_top, z0], // front-top (step bottom edge)
            [hw, 0.0, z0],   // front-bottom (ground)
            [1.0, 0.0, 0.0],
        );
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Generate compound collider for stairs
pub fn generate_stairs_collider(params: &StairsMarker) -> Collider {
    let step_height = params.height / params.step_count as f32;
    let step_depth = params.depth / params.step_count as f32;

    let mut shapes = Vec::new();
    for i in 0..params.step_count {
        let cumulative_height = (i + 1) as f32 * step_height;
        let z_center = -((i as f32 + 0.5) * step_depth);
        let y_center = cumulative_height / 2.0;

        shapes.push((
            Vec3::new(0.0, y_center, z_center),
            Quat::IDENTITY,
            Collider::cuboid(params.width, cumulative_height, step_depth),
        ));
    }

    Collider::compound(shapes)
}

/// Generate mesh for ramp (triangular prism / wedge)
/// Ramp goes from front (z=0, y=0) to back (z=-length, y=height)
pub fn generate_ramp_mesh(params: &RampMarker) -> Mesh {
    let hw = params.width / 2.0;
    let h = params.height;
    let l = params.length;

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // Calculate slope normal (perpendicular to slope surface, pointing up/forward)
    let slope_normal = Vec3::new(0.0, l, h).normalize();

    // Bottom face (-Y normal)
    // Cross product should give -Y
    // Using: front-left → back-left → back-right gives -Y
    add_quad(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        [-hw, 0.0, 0.0], // front-left
        [-hw, 0.0, -l],  // back-left
        [hw, 0.0, -l],   // back-right
        [hw, 0.0, 0.0],  // front-right
        [0.0, -1.0, 0.0],
    );

    // Slope face (angled normal pointing up and toward +Z)
    // Cross product should give (0, l, h) direction
    // Using: front-left → front-right → back-right gives correct slope normal
    add_quad(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        [-hw, 0.0, 0.0], // front-left (bottom)
        [hw, 0.0, 0.0],  // front-right (bottom)
        [hw, h, -l],     // back-right (top)
        [-hw, h, -l],    // back-left (top)
        [slope_normal.x, slope_normal.y, slope_normal.z],
    );

    // Back face (-Z normal)
    // Cross product should give -Z
    // Using: bottom-right(-X) → top-right(-X) → top-left(+X) gives -Z
    add_quad(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        [-hw, 0.0, -l], // bottom-right (from back view)
        [-hw, h, -l],   // top-right
        [hw, h, -l],    // top-left
        [hw, 0.0, -l],  // bottom-left
        [0.0, 0.0, -1.0],
    );

    // Left side triangle (-X normal)
    // Cross product should give -X
    // Using: front → back-top → back-bottom gives -X
    add_triangle(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        [-hw, 0.0, 0.0], // front-bottom
        [-hw, h, -l],    // back-top
        [-hw, 0.0, -l],  // back-bottom
        [-1.0, 0.0, 0.0],
    );

    // Right side triangle (+X normal)
    // Cross product should give +X
    // Using: back-bottom → back-top → front gives +X
    add_triangle(
        &mut positions,
        &mut normals,
        &mut uvs,
        &mut indices,
        [hw, 0.0, -l],  // back-bottom
        [hw, h, -l],    // back-top
        [hw, 0.0, 0.0], // front-bottom
        [1.0, 0.0, 0.0],
    );

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Generate collider for ramp using convex hull
pub fn generate_ramp_collider(params: &RampMarker) -> Collider {
    let hw = params.width / 2.0;

    let vertices = vec![
        Vec3::new(-hw, 0.0, 0.0),
        Vec3::new(hw, 0.0, 0.0),
        Vec3::new(-hw, 0.0, -params.length),
        Vec3::new(hw, 0.0, -params.length),
        Vec3::new(-hw, params.height, -params.length),
        Vec3::new(hw, params.height, -params.length),
    ];

    Collider::convex_hull(vertices).unwrap_or_else(|| {
        Collider::cuboid(params.width, params.height, params.length)
    })
}

/// Generate mesh for arch (wall with semi-circular opening)
pub fn generate_arch_mesh(params: &ArchMarker) -> Mesh {
    let hww = params.wall_width / 2.0;
    let ht = params.thickness / 2.0;
    let how = params.opening_width / 2.0;
    let ar = params.opening_width / 2.0;
    let oh = params.opening_height;
    let wh = params.wall_height;

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // Left pillar
    // Front face (+Z)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [-how, 0.0, ht],
        [-hww, 0.0, ht],
        [-hww, wh, ht],
        [-how, wh, ht],
        [0.0, 0.0, 1.0],
    );
    // Back face (-Z)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [-hww, 0.0, -ht],
        [-how, 0.0, -ht],
        [-how, wh, -ht],
        [-hww, wh, -ht],
        [0.0, 0.0, -1.0],
    );
    // Inner face (+X, facing opening)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [-how, 0.0, -ht],
        [-how, 0.0, ht],
        [-how, oh, ht],
        [-how, oh, -ht],
        [1.0, 0.0, 0.0],
    );
    // Outer face (-X)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [-hww, 0.0, ht],
        [-hww, 0.0, -ht],
        [-hww, wh, -ht],
        [-hww, wh, ht],
        [-1.0, 0.0, 0.0],
    );

    // Right pillar
    // Front face (+Z)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [hww, 0.0, ht],
        [how, 0.0, ht],
        [how, wh, ht],
        [hww, wh, ht],
        [0.0, 0.0, 1.0],
    );
    // Back face (-Z)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [how, 0.0, -ht],
        [hww, 0.0, -ht],
        [hww, wh, -ht],
        [how, wh, -ht],
        [0.0, 0.0, -1.0],
    );
    // Inner face (-X, facing opening)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [how, 0.0, ht],
        [how, 0.0, -ht],
        [how, oh, -ht],
        [how, oh, ht],
        [-1.0, 0.0, 0.0],
    );
    // Outer face (+X)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [hww, 0.0, -ht],
        [hww, 0.0, ht],
        [hww, wh, ht],
        [hww, wh, -ht],
        [1.0, 0.0, 0.0],
    );

    // Top section above arch
    let arch_top = oh + ar;
    // Front face (+Z)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [how, arch_top, ht],
        [-how, arch_top, ht],
        [-how, wh, ht],
        [how, wh, ht],
        [0.0, 0.0, 1.0],
    );
    // Back face (-Z)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [-how, arch_top, -ht],
        [how, arch_top, -ht],
        [how, wh, -ht],
        [-how, wh, -ht],
        [0.0, 0.0, -1.0],
    );

    // Top face of wall (+Y)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [-hww, wh, ht],
        [-hww, wh, -ht],
        [hww, wh, -ht],
        [hww, wh, ht],
        [0.0, 1.0, 0.0],
    );

    // Arch curve segments
    let segments = params.arch_segments.max(4);
    for i in 0..segments {
        let a0 = std::f32::consts::PI * (i as f32 / segments as f32);
        let a1 = std::f32::consts::PI * ((i + 1) as f32 / segments as f32);

        let x0 = -ar * a0.cos();
        let y0 = oh + ar * a0.sin();
        let x1 = -ar * a1.cos();
        let y1 = oh + ar * a1.sin();

        // Inner curve (normals point toward center of opening)
        let nx0 = -a0.cos();
        let ny0 = -a0.sin();
        let nx1 = -a1.cos();
        let ny1 = -a1.sin();

        // Inner arch surface
        let base = positions.len() as u32;
        positions.extend_from_slice(&[
            [x0, y0, ht],
            [x0, y0, -ht],
            [x1, y1, -ht],
            [x1, y1, ht],
        ]);
        normals.extend_from_slice(&[
            [nx0, ny0, 0.0],
            [nx0, ny0, 0.0],
            [nx1, ny1, 0.0],
            [nx1, ny1, 0.0],
        ]);
        uvs.extend_from_slice(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);

        // Front face of arch band (+Z)
        add_quad(
            &mut positions, &mut normals, &mut uvs, &mut indices,
            [x1, y1, ht],
            [x0, y0, ht],
            [x0, arch_top, ht],
            [x1, arch_top, ht],
            [0.0, 0.0, 1.0],
        );

        // Back face of arch band (-Z)
        add_quad(
            &mut positions, &mut normals, &mut uvs, &mut indices,
            [x0, y0, -ht],
            [x1, y1, -ht],
            [x1, arch_top, -ht],
            [x0, arch_top, -ht],
            [0.0, 0.0, -1.0],
        );
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Generate collider for arch (compound of cuboids)
pub fn generate_arch_collider(params: &ArchMarker) -> Collider {
    let hww = params.wall_width / 2.0;
    let how = params.opening_width / 2.0;
    let pillar_width = hww - how;
    let ar = params.opening_width / 2.0;

    let mut shapes = Vec::new();

    // Left pillar
    shapes.push((
        Vec3::new(-hww + pillar_width / 2.0, params.wall_height / 2.0, 0.0),
        Quat::IDENTITY,
        Collider::cuboid(pillar_width, params.wall_height, params.thickness),
    ));

    // Right pillar
    shapes.push((
        Vec3::new(hww - pillar_width / 2.0, params.wall_height / 2.0, 0.0),
        Quat::IDENTITY,
        Collider::cuboid(pillar_width, params.wall_height, params.thickness),
    ));

    // Top section
    let top_height = params.wall_height - (params.opening_height + ar);
    if top_height > 0.0 {
        shapes.push((
            Vec3::new(0.0, params.opening_height + ar + top_height / 2.0, 0.0),
            Quat::IDENTITY,
            Collider::cuboid(params.opening_width, top_height, params.thickness),
        ));
    }

    Collider::compound(shapes)
}

/// Generate mesh for L-shape
pub fn generate_lshape_mesh(params: &LShapeMarker) -> Mesh {
    let w = params.arm_width;
    let h = params.height;
    let l1 = params.arm1_length;
    let l2 = params.arm2_length;

    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    // Arm 1 (along +X, from 0 to l1, z from 0 to w)

    // Front face (+Z at z=w)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [l1, 0.0, w],
        [0.0, 0.0, w],
        [0.0, h, w],
        [l1, h, w],
        [0.0, 0.0, 1.0],
    );

    // Back face (-Z at z=0, only from x=w to x=l1)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [w, 0.0, 0.0],
        [l1, 0.0, 0.0],
        [l1, h, 0.0],
        [w, h, 0.0],
        [0.0, 0.0, -1.0],
    );

    // Top face (+Y, arm1 portion not overlapping with arm2)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [l1, h, w],
        [l1, h, 0.0],
        [w, h, 0.0],
        [w, h, w],
        [0.0, 1.0, 0.0],
    );

    // Bottom face (-Y)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [0.0, 0.0, 0.0],
        [0.0, 0.0, w],
        [l1, 0.0, w],
        [l1, 0.0, 0.0],
        [0.0, -1.0, 0.0],
    );

    // Right end (+X at x=l1)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [l1, 0.0, 0.0],
        [l1, 0.0, w],
        [l1, h, w],
        [l1, h, 0.0],
        [1.0, 0.0, 0.0],
    );

    // Arm 2 (along +Z, from z=w to z=l2, x from 0 to w)

    // Right face (+X at x=w, only from z=w to z=l2)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [w, 0.0, w],
        [w, 0.0, l2],
        [w, h, l2],
        [w, h, w],
        [1.0, 0.0, 0.0],
    );

    // Left face (-X at x=0)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [0.0, 0.0, l2],
        [0.0, 0.0, 0.0],
        [0.0, h, 0.0],
        [0.0, h, l2],
        [-1.0, 0.0, 0.0],
    );

    // Top face (+Y, arm2 portion not overlapping with arm1)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [w, h, l2],
        [0.0, h, l2],
        [0.0, h, w],
        [w, h, w],
        [0.0, 1.0, 0.0],
    );

    // Bottom face (-Y, arm2 portion)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [0.0, 0.0, w],
        [0.0, 0.0, l2],
        [w, 0.0, l2],
        [w, 0.0, w],
        [0.0, -1.0, 0.0],
    );

    // Far end (+Z at z=l2)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [w, 0.0, l2],
        [0.0, 0.0, l2],
        [0.0, h, l2],
        [w, h, l2],
        [0.0, 0.0, 1.0],
    );

    // Corner joint top (covers the 0,0 to w,w area)
    add_quad(
        &mut positions, &mut normals, &mut uvs, &mut indices,
        [w, h, w],
        [0.0, h, w],
        [0.0, h, 0.0],
        [w, h, 0.0],
        [0.0, 1.0, 0.0],
    );

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_indices(Indices::U32(indices))
}

/// Generate collider for L-shape (compound of two cuboids)
pub fn generate_lshape_collider(params: &LShapeMarker) -> Collider {
    let w = params.arm_width;
    let h = params.height;
    let l1 = params.arm1_length;
    let l2 = params.arm2_length;

    let shapes = vec![
        // Arm 1 (along +X)
        (
            Vec3::new(l1 / 2.0, h / 2.0, w / 2.0),
            Quat::IDENTITY,
            Collider::cuboid(l1, h, w),
        ),
        // Arm 2 (along +Z, excluding the corner overlap)
        (
            Vec3::new(w / 2.0, h / 2.0, w + (l2 - w) / 2.0),
            Quat::IDENTITY,
            Collider::cuboid(w, h, l2 - w),
        ),
    ];

    Collider::compound(shapes)
}
