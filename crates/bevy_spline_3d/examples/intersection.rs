//! Road intersection example.
//!
//! Demonstrates creating intersections where multiple roads meet.
//!
//! Run with: `cargo run --example intersection`

use bevy::prelude::*;
use bevy_spline_3d::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(SplinePlugin)
        .add_plugins(SplineEditorPlugin)
        .add_plugins(SplineRoadPlugin)
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
        Transform::from_xyz(0.0, 25.0, 25.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            focus: Vec3::ZERO,
            radius: 35.0,
            pitch: 0.8,
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

    // Road materials
    let road_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.3, 0.3, 0.35),
        perceptual_roughness: 0.9,
        ..default()
    });

    let intersection_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.35, 0.35, 0.4),
        perceptual_roughness: 0.9,
        ..default()
    });

    // Create road segment mesh
    let road_segment = create_road_segment_mesh(4.0, 2.0, 0.15, 0.3);
    let segment_handle = meshes.add(road_segment);

    // Create 4 splines meeting at center (crossroad)
    // North road
    let north_spline = commands
        .spawn(Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(0.0, 0.0, 20.0),
                Vec3::new(0.0, 0.0, 15.0),
                Vec3::new(0.0, 0.0, 5.0),
                Vec3::new(0.0, 0.0, 2.0),
            ],
        ))
        .id();

    // South road
    let south_spline = commands
        .spawn(Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(0.0, 0.0, -20.0),
                Vec3::new(0.0, 0.0, -15.0),
                Vec3::new(0.0, 0.0, -5.0),
                Vec3::new(0.0, 0.0, -2.0),
            ],
        ))
        .id();

    // East road
    let east_spline = commands
        .spawn(Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(20.0, 0.0, 0.0),
                Vec3::new(15.0, 0.0, 0.0),
                Vec3::new(5.0, 0.0, 0.0),
                Vec3::new(2.0, 0.0, 0.0),
            ],
        ))
        .id();

    // West road
    let west_spline = commands
        .spawn(Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(-20.0, 0.0, 0.0),
                Vec3::new(-15.0, 0.0, 0.0),
                Vec3::new(-5.0, 0.0, 0.0),
                Vec3::new(-2.0, 0.0, 0.0),
            ],
        ))
        .id();

    // Create road entities
    let north_road = commands
        .spawn((
            SplineRoad::new(north_spline, segment_handle.clone()).with_segments(16),
            MeshMaterial3d(road_material.clone()),
            Transform::default(),
            Visibility::default(),
        ))
        .id();

    let south_road = commands
        .spawn((
            SplineRoad::new(south_spline, segment_handle.clone()).with_segments(16),
            MeshMaterial3d(road_material.clone()),
            Transform::default(),
            Visibility::default(),
        ))
        .id();

    let east_road = commands
        .spawn((
            SplineRoad::new(east_spline, segment_handle.clone()).with_segments(16),
            MeshMaterial3d(road_material.clone()),
            Transform::default(),
            Visibility::default(),
        ))
        .id();

    let west_road = commands
        .spawn((
            SplineRoad::new(west_spline, segment_handle.clone()).with_segments(16),
            MeshMaterial3d(road_material.clone()),
            Transform::default(),
            Visibility::default(),
        ))
        .id();

    // Create intersection where all roads meet
    // Each road's End connects to the intersection (since splines go toward center)
    commands.spawn((
        RoadIntersection::new(vec![
            RoadConnection::end(north_road),
            RoadConnection::end(east_road),
            RoadConnection::end(south_road),
            RoadConnection::end(west_road),
        ]),
        MeshMaterial3d(intersection_material),
        Transform::default(),
        Visibility::default(),
    ));

    // Ground plane
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(50.0)))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.5, 0.2),
            perceptual_roughness: 1.0,
            ..default()
        })),
        Transform::from_xyz(0.0, -0.1, 0.0),
    ));

    println!("\n=== Road Intersection Example ===");
    println!("A crossroad intersection with 4 roads meeting at the center.");
    println!();
    println!("Controls:");
    println!("  Right-click + drag - Orbit camera");
    println!("  Scroll wheel       - Zoom");
    println!("  F                  - Toggle fly/orbit camera");
    println!("=================================\n");
}
