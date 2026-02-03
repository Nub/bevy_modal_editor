//! Path following example.
//!
//! Demonstrates entities following spline paths with different loop modes.
//!
//! Run with: `cargo run --example path_follow`

use bevy::prelude::*;
use bevy_spline_3d::prelude::*;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(SplinePlugin)
        .add_plugins(SplineEditorPlugin)
        .add_plugins(SplineFollowPlugin)
        .add_plugins(CameraPlugin)
        .add_systems(Startup, setup)
        .add_systems(Update, (handle_input, log_follower_events))
        .run();
}

#[derive(Component)]
pub struct FollowerLabel(&'static str);

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Camera
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(0.0, 15.0, 20.0).looking_at(Vec3::ZERO, Vec3::Y),
        OrbitCamera {
            focus: Vec3::new(0.0, 0.0, 0.0),
            radius: 25.0,
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
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Create a figure-8 spline (closed loop)
    let spline_entity = commands
        .spawn(Spline::closed(
            SplineType::CatmullRom,
            vec![
                Vec3::new(-8.0, 0.0, 0.0),
                Vec3::new(-4.0, 0.0, -6.0),
                Vec3::new(0.0, 2.0, 0.0),
                Vec3::new(4.0, 0.0, 6.0),
                Vec3::new(8.0, 0.0, 0.0),
                Vec3::new(4.0, 0.0, -6.0),
                Vec3::new(0.0, -2.0, 0.0),
                Vec3::new(-4.0, 0.0, 6.0),
            ],
        ))
        .id();

    // Shared mesh and materials
    let cube_mesh = meshes.add(Cuboid::new(0.8, 0.8, 1.2));
    let sphere_mesh = meshes.add(Sphere::new(0.5));

    let red_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.9, 0.2, 0.2),
        ..default()
    });
    let green_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.9, 0.2),
        ..default()
    });
    let blue_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.2, 0.2, 0.9),
        ..default()
    });

    // Follower 1: Loop mode (red cube)
    commands.spawn((
        Mesh3d(cube_mesh.clone()),
        MeshMaterial3d(red_material),
        Transform::default(),
        SplineFollower::new(spline_entity)
            .with_speed(5.0)
            .with_loop_mode(LoopMode::Loop)
            .with_start_t(0.0),
        FollowerLabel("Loop"),
    ));

    // Follower 2: PingPong mode (green cube)
    commands.spawn((
        Mesh3d(cube_mesh.clone()),
        MeshMaterial3d(green_material),
        Transform::default(),
        SplineFollower::new(spline_entity)
            .with_speed(3.0)
            .with_loop_mode(LoopMode::PingPong)
            .with_start_t(0.25),
        FollowerLabel("PingPong"),
    ));

    // Follower 3: Once mode (blue sphere) - will stop at end
    commands.spawn((
        Mesh3d(sphere_mesh),
        MeshMaterial3d(blue_material),
        Transform::default(),
        SplineFollower::new(spline_entity)
            .with_speed(2.0)
            .with_loop_mode(LoopMode::Once)
            .with_start_t(0.5)
            .with_align_to_tangent(false), // Sphere doesn't rotate
        FollowerLabel("Once"),
    ));

    // Ground plane for reference
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(20.0)))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.3, 0.3),
            ..default()
        })),
        Transform::from_xyz(0.0, -3.0, 0.0),
    ));

    println!("\n=== Path Following Example ===");
    println!("Red cube: Loop mode (continuous)");
    println!("Green cube: PingPong mode (bounces)");
    println!("Blue sphere: Once mode (stops at end)");
    println!();
    println!("Controls:");
    println!("  Space  - Pause/Resume all followers");
    println!("  R      - Reset all followers");
    println!("  F      - Toggle camera mode");
    println!("================================\n");
}

fn handle_input(keyboard: Res<ButtonInput<KeyCode>>, mut followers: Query<&mut SplineFollower>) {
    // Space to toggle pause
    if keyboard.just_pressed(KeyCode::Space) {
        for mut follower in &mut followers {
            if follower.is_playing() {
                follower.pause();
                println!("Paused");
            } else {
                follower.play();
                println!("Playing");
            }
        }
    }

    // R to reset
    if keyboard.just_pressed(KeyCode::KeyR) {
        for mut follower in &mut followers {
            follower.reset();
        }
        println!("Reset all followers");
    }
}

fn log_follower_events(mut events: MessageReader<FollowerEvent>, labels: Query<&FollowerLabel>) {
    for event in events.read() {
        let label = labels.get(event.entity).map(|l| l.0).unwrap_or("Unknown");

        match event.kind {
            FollowerEventKind::ReachedEnd => {
                println!("[{}] Reached end", label);
            }
            FollowerEventKind::ReachedStart => {
                println!("[{}] Reached start", label);
            }
            FollowerEventKind::LoopCompleted => {
                println!("[{}] Loop completed", label);
            }
            FollowerEventKind::Finished => {
                println!("[{}] Finished!", label);
            }
        }
    }
}
