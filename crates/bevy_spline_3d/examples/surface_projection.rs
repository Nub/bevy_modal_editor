//! Surface projection example.
//!
//! Demonstrates roads and distributed objects conforming to terrain.
//!
//! Run with: `cargo run --example surface_projection --features surface_projection`

use avian3d::prelude::*;
use bevy::{light::DirectionalLightShadowMap, prelude::*};
use bevy_spline_3d::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Surface Projection Example".into(),
                ..default()
            }),
            ..default()
        }))
        // Higher resolution shadow map for better self-shadowing on roads
        .insert_resource(DirectionalLightShadowMap { size: 4096 })
        .add_plugins(PhysicsPlugins::default())
        .add_plugins(SplinePlugin)
        .add_plugins(SplineEditorPlugin)
        .add_plugins(SplineRoadPlugin)
        .add_plugins(SplineDistributionPlugin)
        .add_plugins(SurfaceProjectionPlugin)
        .add_plugins(CameraPlugin)
        .add_systems(Startup, setup)
        .run();
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 30.0, 40.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            focus: Vec3::ZERO,
            radius: 50.0,
            pitch: 0.6,
            ..default()
        },
        FlyCamera::default(),
    ));

    // Lighting
    commands.spawn(AmbientLight {
        color: Color::WHITE,
        brightness: 300.0,
        ..default()
    });

    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: true,
            // Lower bias values for better self-shadowing on roads
            shadow_depth_bias: 0.005,
            shadow_normal_bias: 0.5,
            ..default()
        },
        Transform::from_xyz(10.0, 30.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Create hilly terrain mesh
    let terrain_mesh = create_terrain_mesh(50.0, 50.0, 32, 32);
    let terrain_handle = meshes.add(terrain_mesh.clone());

    let terrain_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.5, 0.2),
        perceptual_roughness: 0.9,
        ..default()
    });

    // Spawn terrain with collider for raycasting
    commands.spawn((
        Mesh3d(terrain_handle),
        MeshMaterial3d(terrain_material),
        Transform::default(),
        RigidBody::Static,
        Collider::trimesh_from_mesh(&terrain_mesh).unwrap(),
        CollisionLayers::new(ProjectionLayer::Terrain, LayerMask::ALL),
    ));

    // Road material
    let road_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.3, 0.35),
        perceptual_roughness: 0.9,
        ..default()
    });

    // Create road segment mesh
    let road_segment = create_road_segment_mesh(4.0, 1.0, 0.1, 0.2);
    let segment_handle = meshes.add(road_segment);

    // Create a winding spline across the terrain
    let spline = commands
        .spawn(Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(-20.0, 5.0, -20.0),
                Vec3::new(-10.0, 5.0, -10.0),
                Vec3::new(0.0, 5.0, 0.0),
                Vec3::new(10.0, 5.0, 10.0),
                Vec3::new(20.0, 5.0, 20.0),
            ],
        ))
        .id();

    // Spawn road with surface projection
    commands.spawn((
        SplineRoad::new(spline, segment_handle).with_segments(64),
        MeshMaterial3d(road_material),
        Transform::default(),
        Visibility::default(),
        SurfaceProjection::new()
            .with_ray_offset(20.0)
            .with_normal_offset(0.15),
    ));

    // === Second road with more complex geometry ===
    let complex_road_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.4, 0.35, 0.3),
        perceptual_roughness: 0.85,
        ..default()
    });

    // Create a road with sidewalks (more complex cross-section)
    let complex_segment = create_road_with_sidewalks(6.0, 1.0, 0.15, 1.0);
    let complex_handle = meshes.add(complex_segment);

    // Create a different winding spline
    let complex_spline = commands
        .spawn(Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(-20.0, 5.0, 10.0),
                Vec3::new(-8.0, 5.0, 5.0),
                Vec3::new(0.0, 5.0, -5.0),
                Vec3::new(12.0, 5.0, -10.0),
                Vec3::new(20.0, 5.0, -15.0),
            ],
        ))
        .id();

    // Spawn the complex road with surface projection
    commands.spawn((
        SplineRoad::new(complex_spline, complex_handle).with_segments(48),
        MeshMaterial3d(complex_road_material),
        Transform::default(),
        Visibility::default(),
        SurfaceProjection::new()
            .with_ray_offset(20.0)
            .with_normal_offset(0.15),
    ));

    // === Third road with prominent curbs ===
    let curb_road_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.35, 0.4),
        perceptual_roughness: 0.85,
        ..default()
    });

    // Create a road with prominent curbs using the built-in function
    // Parameters: width=5.0, segment_length=1.0, curb_height=0.25, curb_width=0.4
    let curb_segment = create_road_segment_mesh(5.0, 1.0, 0.25, 0.4);
    let curb_handle = meshes.add(curb_segment);

    // Create a curved spline for the curbed road
    let curb_spline = commands
        .spawn(Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(15.0, 5.0, -20.0),
                Vec3::new(10.0, 5.0, -8.0),
                Vec3::new(5.0, 5.0, 5.0),
                Vec3::new(-5.0, 5.0, 12.0),
                Vec3::new(-15.0, 5.0, 18.0),
            ],
        ))
        .id();

    // Spawn the curbed road with surface projection
    commands.spawn((
        SplineRoad::new(curb_spline, curb_handle).with_segments(56),
        MeshMaterial3d(curb_road_material),
        Transform::default(),
        Visibility::default(),
        SurfaceProjection::new()
            .with_ray_offset(20.0)
            .with_normal_offset(0.12),
    ));

    // Create a template for distributed objects (posts/markers)
    let post_mesh = meshes.add(Cylinder::new(0.1, 1.0));
    let post_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.4, 0.2),
        ..default()
    });

    let post_template = commands
        .spawn((
            Mesh3d(post_mesh),
            MeshMaterial3d(post_material),
            DistributionSource,
        ))
        .id();

    // Create another spline for posts
    let post_spline = commands
        .spawn(Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(-15.0, 5.0, -15.0),
                Vec3::new(-5.0, 5.0, 5.0),
                Vec3::new(5.0, 5.0, -5.0),
                Vec3::new(15.0, 5.0, 15.0),
            ],
        ))
        .id();

    // Distribute posts along the spline with surface projection
    commands.spawn((
        SplineDistribution {
            spline: post_spline,
            source: post_template,
            count: 20,
            enabled: true,
            orientation: DistributionOrientation::PositionOnly,
            spacing: DistributionSpacing::Uniform,
            offset: Vec3::new(0.0, 0.5, 0.0), // Offset up so post sits on surface
        },
        SurfaceProjection::new()
            .with_ray_offset(20.0)
            .with_normal_alignment(true),
    ));

    println!("\n=== Surface Projection Example ===");
    println!("Road and posts conform to the hilly terrain.");
    println!();
    println!("Camera Controls:");
    println!("  Right-click + drag - Orbit camera");
    println!("  Scroll wheel       - Zoom");
    println!("  F                  - Toggle fly/orbit camera");
    println!();
    println!("Editor Controls:");
    println!("  Left-click         - Select spline/control point");
    println!("  G                  - Grab/move selected point");
    println!("  A                  - Add control point");
    println!("  X                  - Delete selected point");
    println!("  Tab                - Cycle spline type");
    println!("  Escape             - Deselect");
    println!("====================================\n");
}

/// Create a hilly terrain mesh.
fn create_terrain_mesh(width: f32, depth: f32, segments_x: usize, segments_z: usize) -> Mesh {
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    let mut uvs = Vec::new();
    let mut indices = Vec::new();

    let half_width = width / 2.0;
    let half_depth = depth / 2.0;

    // Generate vertices
    for z in 0..=segments_z {
        for x in 0..=segments_x {
            let px = (x as f32 / segments_x as f32) * width - half_width;
            let pz = (z as f32 / segments_z as f32) * depth - half_depth;

            // Create hills using sine waves
            let height = (px * 0.2).sin() * 2.0
                + (pz * 0.15).cos() * 2.5
                + ((px + pz) * 0.1).sin() * 1.5;

            positions.push([px, height, pz]);
            normals.push([0.0, 1.0, 0.0]); // Will be recalculated
            uvs.push([x as f32 / segments_x as f32, z as f32 / segments_z as f32]);
        }
    }

    // Generate indices
    for z in 0..segments_z {
        for x in 0..segments_x {
            let top_left = z * (segments_x + 1) + x;
            let top_right = top_left + 1;
            let bottom_left = (z + 1) * (segments_x + 1) + x;
            let bottom_right = bottom_left + 1;

            // Two triangles per quad
            indices.push(top_left as u32);
            indices.push(bottom_left as u32);
            indices.push(top_right as u32);

            indices.push(top_right as u32);
            indices.push(bottom_left as u32);
            indices.push(bottom_right as u32);
        }
    }

    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));

    // Compute proper normals
    mesh.compute_normals();

    mesh
}

/// Create a road segment with raised sidewalks on both sides.
///
/// Cross-section profile:
/// ```text
///   ___________         ___________
///  |  sidewalk |       |  sidewalk |
///  |___________|_______|___________|
///              | road  |
/// ```
fn create_road_with_sidewalks(
    total_width: f32,
    segment_length: f32,
    sidewalk_height: f32,
    sidewalk_width: f32,
) -> Mesh {
    let hw = total_width / 2.0;
    let road_hw = hw - sidewalk_width;

    // Cross-section profile from left to right:
    // left sidewalk outer -> left sidewalk inner (top) -> left sidewalk inner (bottom/road level)
    // -> road left -> road right
    // -> right sidewalk inner (bottom) -> right sidewalk inner (top) -> right sidewalk outer
    let profile = vec![
        // Left sidewalk - outer edge top
        Vec3::new(-hw, sidewalk_height, 0.0),
        // Left sidewalk - inner edge top
        Vec3::new(-road_hw, sidewalk_height, 0.0),
        // Left sidewalk - inner edge bottom (curb face)
        Vec3::new(-road_hw, 0.0, 0.0),
        // Road surface - left
        Vec3::new(-road_hw + 0.05, 0.0, 0.0),
        // Road surface - right
        Vec3::new(road_hw - 0.05, 0.0, 0.0),
        // Right sidewalk - inner edge bottom (curb face)
        Vec3::new(road_hw, 0.0, 0.0),
        // Right sidewalk - inner edge top
        Vec3::new(road_hw, sidewalk_height, 0.0),
        // Right sidewalk - outer edge top
        Vec3::new(hw, sidewalk_height, 0.0),
    ];

    let profile_len = profile.len();

    // Generate vertices for front and back edges
    let mut positions = Vec::with_capacity(profile_len * 2);
    let mut normals = Vec::with_capacity(profile_len * 2);
    let mut uvs = Vec::with_capacity(profile_len * 2);

    // Front edge (Z=0)
    for (i, p) in profile.iter().enumerate() {
        positions.push([p.x, p.y, 0.0]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([i as f32 / (profile_len - 1) as f32, 0.0]);
    }

    // Back edge (Z=segment_length)
    for (i, p) in profile.iter().enumerate() {
        positions.push([p.x, p.y, segment_length]);
        normals.push([0.0, 1.0, 0.0]);
        uvs.push([i as f32 / (profile_len - 1) as f32, 1.0]);
    }

    // Generate indices for triangles between front and back
    let mut indices = Vec::new();
    for i in 0..(profile_len - 1) {
        let front_left = i as u32;
        let front_right = (i + 1) as u32;
        let back_left = (i + profile_len) as u32;
        let back_right = (i + 1 + profile_len) as u32;

        // Two triangles per quad (CW winding for upward-facing)
        indices.extend_from_slice(&[front_left, front_right, back_left]);
        indices.extend_from_slice(&[front_right, back_right, back_left]);
    }

    let mut mesh = Mesh::new(
        bevy::mesh::PrimitiveTopology::TriangleList,
        default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(bevy::mesh::Indices::U32(indices));

    mesh
}
