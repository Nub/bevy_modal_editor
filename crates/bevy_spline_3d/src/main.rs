//! Example spline editor application.
//!
//! Run with: `cargo run`

use bevy::prelude::*;
use bevy_spline_3d::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(SplinePlugin)
        .add_plugins(SplineEditorPlugin)
        .add_plugins(CameraPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, show_help)
        .run();
}

fn setup(mut commands: Commands) {
    // Camera with orbit and fly controls
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(5.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera::default(),
        FlyCamera::default(),
    ));

    // Ambient light
    commands.spawn(AmbientLight {
        color: Color::WHITE,
        brightness: 500.0,
        affects_lightmapped_meshes: true,
    });

    // Directional light
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Ground plane reference (using gizmos in a system instead)

    // Example Catmull-Rom spline
    commands.spawn(Spline::new(
        SplineType::CatmullRom,
        vec![
            Vec3::new(-4.0, 0.0, 0.0),
            Vec3::new(-2.0, 2.0, 1.0),
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(2.0, 1.5, 0.5),
            Vec3::new(4.0, 0.0, 0.0),
        ],
    ));

    // Example Cubic Bézier spline
    commands.spawn(Spline::new(
        SplineType::CubicBezier,
        vec![
            // First segment
            Vec3::new(-3.0, 0.0, 3.0), // Anchor
            Vec3::new(-2.0, 2.0, 3.0), // Handle
            Vec3::new(-1.0, 2.0, 3.0), // Handle
            Vec3::new(0.0, 0.0, 3.0),  // Anchor
            // Second segment
            Vec3::new(1.0, -2.0, 3.0), // Handle
            Vec3::new(2.0, -2.0, 3.0), // Handle
            Vec3::new(3.0, 0.0, 3.0),  // Anchor
        ],
    ));
}

fn show_help(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut shown: Local<bool>,
    camera_mode: Res<CameraMode>,
    editor_settings: Res<EditorSettings>,
) {
    if keyboard.just_pressed(KeyCode::KeyH) {
        *shown = !*shown;
    }

    if keyboard.just_pressed(KeyCode::KeyH) || camera_mode.is_changed() {
        if *shown {
            println!("\n=== Spline Editor Help ===");
            println!("Camera Mode: {}", camera_mode.name());
            println!("Editor: {}", if editor_settings.enabled { "Enabled" } else { "Disabled" });
            println!();
            println!("Controls:");
            println!("  H         - Toggle this help");
            println!("  F         - Toggle camera mode (Orbit/Fly)");
            println!("  A         - Add control point");
            println!("  X         - Delete selected point");
            println!("  Tab       - Cycle spline type");
            println!("  C         - Toggle closed/open");
            println!("  Escape    - Deselect all");
            println!();
            println!("Camera (Orbit):");
            println!("  RMB + drag - Orbit");
            println!("  Scroll     - Zoom");
            println!();
            println!("Camera (Fly):");
            println!("  RMB + drag - Look");
            println!("  WASD       - Move");
            println!("  Q/Space    - Up");
            println!("  E/Ctrl     - Down");
            println!("  Shift      - Sprint");
            println!("========================\n");
        }
    }
}
