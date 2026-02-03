//! Example demonstrating road mesh generation along splines.
//!
//! Run with: `cargo run --example road`

use bevy::prelude::*;
use bevy_spline_3d::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Spline Road Example".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(SplinePlugin)
        .add_plugins(SplineRoadPlugin)
        .add_plugins(SplineEditorPlugin)
        .add_plugins(CameraPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, draw_grid)
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
        Transform::from_xyz(15.0, 12.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            focus: Vec3::ZERO,
            radius: 25.0,
            ..default()
        },
        FlyCamera::default(),
    ));

    // Lighting
    commands.spawn(AmbientLight {
        color: Color::WHITE,
        brightness: 400.0,
        affects_lightmapped_meshes: true,
    });

    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(10.0, 20.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Create a curved road spline
    let road_spline = commands
        .spawn(Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(-15.0, 0.0, -5.0),
                Vec3::new(-8.0, 0.0, -5.0),
                Vec3::new(-4.0, 0.0, 0.0),
                Vec3::new(0.0, 2.0, 5.0),   // Hill
                Vec3::new(4.0, 0.0, 8.0),
                Vec3::new(10.0, 0.0, 6.0),
                Vec3::new(15.0, 0.0, 10.0),
            ],
        ))
        .id();

    // Create road segment mesh with curbs
    // This is a cross-section that will be extruded along the spline
    let road_segment = create_road_segment_mesh(
        4.0,  // width
        1.0,  // segment_length (doesn't matter much, just for the template)
        0.15, // curb_height
        0.3,  // curb_width
    );
    let road_segment_handle = meshes.add(road_segment);

    // Road material (asphalt-like)
    let road_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.15, 0.15, 0.17),
        perceptual_roughness: 0.9,
        ..default()
    });

    // Spawn the road
    commands.spawn((
        SplineRoad::new(road_spline, road_segment_handle.clone())
            .with_segments(64)
            .with_uv_tile(8.0),
        MeshMaterial3d(road_material.clone()),
        Transform::default(),
        Visibility::Inherited,
    ));

    // Create a second road (simpler, no curbs) for a path
    let path_spline = commands
        .spawn(Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(-5.0, 0.01, -12.0),
                Vec3::new(-3.0, 0.01, -8.0),
                Vec3::new(0.0, 0.01, -6.0),
                Vec3::new(5.0, 0.01, -8.0),
                Vec3::new(8.0, 0.01, -12.0),
            ],
        ))
        .id();

    let path_segment = create_road_segment_mesh(
        1.5, // narrower
        1.0,
        0.0, // no curbs
        0.0,
    );
    let path_segment_handle = meshes.add(path_segment);

    let path_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.6, 0.5, 0.4), // dirt path color
        perceptual_roughness: 1.0,
        ..default()
    });

    commands.spawn((
        SplineRoad::new(path_spline, path_segment_handle)
            .with_segments(32)
            .with_uv_tile(4.0),
        MeshMaterial3d(path_material),
        Transform::default(),
        Visibility::Inherited,
    ));

    // Ground plane
    let ground = meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(50.0)));
    let ground_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.35, 0.15),
        perceptual_roughness: 1.0,
        ..default()
    });
    commands.spawn((
        Mesh3d(ground),
        MeshMaterial3d(ground_material),
        Transform::from_xyz(0.0, -0.01, 0.0),
    ));

    println!("Spline Road Example");
    println!("-------------------");
    println!("Dark road: 4m wide with curbs, follows terrain");
    println!("Brown path: 1.5m wide, flat dirt path");
    println!();
    println!("Controls:");
    println!("  Click + drag control points to reshape roads");
    println!("  Road meshes update automatically");
    println!("  F - Toggle camera mode");
    println!();
    println!("Mesh Requirements for Custom Roads:");
    println!("  - Cross-section oriented along Z axis");
    println!("  - Width along X, height along Y");
    println!("  - Front vertices at Z=0, back at Z>0");
    println!("  - Consistent vertex count at both ends");
}

fn draw_grid(mut gizmos: Gizmos) {
    let grid_size = 20;
    let grid_color = Color::srgba(0.2, 0.2, 0.2, 0.3);

    for i in -grid_size..=grid_size {
        let pos = i as f32;
        gizmos.line(
            Vec3::new(pos, 0.001, -grid_size as f32),
            Vec3::new(pos, 0.001, grid_size as f32),
            grid_color,
        );
        gizmos.line(
            Vec3::new(-grid_size as f32, 0.001, pos),
            Vec3::new(grid_size as f32, 0.001, pos),
            grid_color,
        );
    }
}
