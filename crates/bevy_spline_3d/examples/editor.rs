//! Full-featured spline editor example.
//!
//! Demonstrates all features of bevy_spline_3d including:
//! - Multiple spline types
//! - Interactive editing
//! - Camera controls
//! - Scene saving/loading
//!
//! Run with: `cargo run --example editor`

use bevy::prelude::*;
use bevy_spline_3d::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy Spline 3D Editor".into(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(SplinePlugin)
        .add_plugins(SplineEditorPlugin)
        .add_plugins(CameraPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, (draw_grid, ui_overlay))
        .run();
}

fn setup(mut commands: Commands) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(8.0, 6.0, 12.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            focus: Vec3::ZERO,
            radius: 15.0,
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

    // Spawn example splines
    spawn_example_splines(&mut commands);
}

fn spawn_example_splines(commands: &mut Commands) {
    // Catmull-Rom spiral
    let mut spiral_points = Vec::new();
    for i in 0..8 {
        let angle = i as f32 * std::f32::consts::TAU / 8.0;
        let radius = 2.0 + i as f32 * 0.3;
        let height = i as f32 * 0.4;
        spiral_points.push(Vec3::new(
            angle.cos() * radius,
            height,
            angle.sin() * radius,
        ));
    }
    commands.spawn(Spline::new(SplineType::CatmullRom, spiral_points));

    // B-Spline wave
    let wave_points: Vec<Vec3> = (0..6)
        .map(|i| {
            Vec3::new(
                -4.0 + i as f32 * 1.5,
                (i as f32 * 1.5).sin() * 1.5,
                -3.0,
            )
        })
        .collect();
    commands.spawn(Spline::new(SplineType::BSpline, wave_points));

    // Closed Bézier circle approximation
    let r = 2.0;
    let k = 0.5523; // Magic number for circular Bézier approximation
    commands.spawn(Spline::closed(
        SplineType::CubicBezier,
        vec![
            // Right
            Vec3::new(r, 0.0, -5.0),
            Vec3::new(r, k * r, -5.0),
            Vec3::new(k * r, r, -5.0),
            // Top
            Vec3::new(0.0, r, -5.0),
            Vec3::new(-k * r, r, -5.0),
            Vec3::new(-r, k * r, -5.0),
            // Left
            Vec3::new(-r, 0.0, -5.0),
            Vec3::new(-r, -k * r, -5.0),
            Vec3::new(-k * r, -r, -5.0),
            // Bottom
            Vec3::new(0.0, -r, -5.0),
            Vec3::new(k * r, -r, -5.0),
            Vec3::new(r, -k * r, -5.0),
        ],
    ));
}

fn draw_grid(mut gizmos: Gizmos) {
    let grid_size = 10;
    let grid_color = Color::srgba(0.3, 0.3, 0.3, 0.5);

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

    // Axis indicators
    gizmos.line(Vec3::ZERO, Vec3::X * 2.0, Color::srgb(1.0, 0.2, 0.2));
    gizmos.line(Vec3::ZERO, Vec3::Y * 2.0, Color::srgb(0.2, 1.0, 0.2));
    gizmos.line(Vec3::ZERO, Vec3::Z * 2.0, Color::srgb(0.2, 0.2, 1.0));
}

fn ui_overlay(
    camera_mode: Res<CameraMode>,
    _editor_settings: Res<EditorSettings>,
    splines: Query<Ref<Spline>, With<SelectedSpline>>,
) {
    // This would typically use bevy_egui for a proper UI overlay
    // For now, changes are logged to console

    if camera_mode.is_changed() {
        println!("Camera mode: {}", camera_mode.name());
    }

    for spline in &splines {
        if spline.is_changed() {
            println!(
                "Selected spline: {} ({} points, {})",
                spline.spline_type.name(),
                spline.control_points.len(),
                if spline.closed { "closed" } else { "open" }
            );
        }
    }
}
