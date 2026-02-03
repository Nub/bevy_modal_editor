//! Example demonstrating entity distribution along splines.
//!
//! Run with: `cargo run --example distribution`

use bevy::prelude::*;
use bevy_spline_3d::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Spline Distribution Example".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(SplinePlugin)
        .add_plugins(SplineDistributionPlugin)
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
        Transform::from_xyz(10.0, 8.0, 15.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            focus: Vec3::ZERO,
            radius: 20.0,
            ..default()
        },
        FlyCamera::default(),
    ));

    // Lighting
    commands.spawn(AmbientLight {
        color: Color::WHITE,
        brightness: 300.0,
        affects_lightmapped_meshes: true,
    });

    commands.spawn((
        DirectionalLight {
            illuminance: 8000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(5.0, 10.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Create a curved spline path
    let spline = commands
        .spawn(Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(-8.0, 0.0, 0.0),
                Vec3::new(-4.0, 2.0, 3.0),
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(4.0, 3.0, -2.0),
                Vec3::new(8.0, 1.0, 1.0),
            ],
        ))
        .id();

    // Create template entity (a small cone-like shape pointing forward)
    let template_mesh = meshes.add(Cone::new(0.2, 0.5));
    let template_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.7, 0.3),
        ..default()
    });

    let template = commands
        .spawn((
            Mesh3d(template_mesh),
            MeshMaterial3d(template_material),
            Transform::default(),
            DistributionSource, // Mark as template (will be hidden)
        ))
        .id();

    // Create distribution - cones aligned to spline direction
    commands.spawn(
        SplineDistribution::new(spline, template, 15)
            .with_orientation(DistributionOrientation::align_to_tangent())
            .with_offset(Vec3::Y * 0.25), // Slight offset above spline
    );

    // Create another spline for position-only distribution
    let spline2 = commands
        .spawn(Spline::closed(
            SplineType::CatmullRom,
            vec![
                Vec3::new(-3.0, 0.0, -8.0),
                Vec3::new(3.0, 0.0, -8.0),
                Vec3::new(3.0, 0.0, -12.0),
                Vec3::new(-3.0, 0.0, -12.0),
            ],
        ))
        .id();

    // Sphere template
    let sphere_mesh = meshes.add(Sphere::new(0.3));
    let sphere_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.3, 0.3),
        ..default()
    });

    let sphere_template = commands
        .spawn((
            Mesh3d(sphere_mesh),
            MeshMaterial3d(sphere_material),
            Transform::default(),
            DistributionSource,
        ))
        .id();

    // Position-only distribution (spheres stay upright)
    commands.spawn(
        SplineDistribution::new(spline2, sphere_template, 12)
            .with_orientation(DistributionOrientation::PositionOnly)
            .with_offset(Vec3::Y * 0.3),
    );

    println!("Spline Distribution Example");
    println!("----------------------------");
    println!("Green cones: Aligned to spline tangent");
    println!("Red spheres: Position only (closed loop)");
    println!();
    println!("Controls:");
    println!("  Click + drag control points to modify splines");
    println!("  Distributed objects update automatically");
    println!("  F - Toggle camera mode");
}

fn draw_grid(mut gizmos: Gizmos) {
    let grid_size = 15;
    let grid_color = Color::srgba(0.3, 0.3, 0.3, 0.3);

    for i in -grid_size..=grid_size {
        let pos = i as f32;
        gizmos.line(
            Vec3::new(pos, 0.0, -grid_size as f32),
            Vec3::new(pos, 0.0, grid_size as f32),
            grid_color,
        );
        gizmos.line(
            Vec3::new(-grid_size as f32, 0.0, pos),
            Vec3::new(grid_size as f32, 0.0, pos),
            grid_color,
        );
    }
}
