//! Basic example demonstrating the world-space grid shader.
//!
//! Run with: `cargo run --example basic`
//!
//! Controls:
//!   - Left click (hold) or M (toggle): Grab cursor for camera control
//!   - WASD: Move horizontally
//!   - E/Q: Move up/down
//!   - Shift: Move faster
//!   - Mouse scroll: Adjust movement speed

use bevy::{camera_controller::free_camera::{FreeCamera, FreeCameraPlugin}, prelude::*};
use bevy_grid_shader::{ExtendedMaterial, GridAxes, GridMaterial, GridMaterialPlugin, StandardMaterial};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, GridMaterialPlugin, FreeCameraPlugin))
        .add_systems(Startup, setup)
        .add_systems(Update, rotate_objects)
        .run();
}

#[derive(Component)]
struct Rotating;

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<ExtendedMaterial<StandardMaterial, GridMaterial>>>,
) {
    // Ground plane with XZ grid
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(20.0, 20.0))),
        MeshMaterial3d(materials.add(ExtendedMaterial {
            base: StandardMaterial {
                base_color: Color::srgb(0.2, 0.2, 0.25),
                perceptual_roughness: 0.9,
                ..default()
            },
            extension: GridMaterial::new()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));

    // Rotating cube with all-axis grid
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(2.0, 2.0, 2.0))),
        MeshMaterial3d(materials.add(ExtendedMaterial {
            base: StandardMaterial {
                base_color: Color::srgb(0.8, 0.3, 0.2),
                perceptual_roughness: 0.3,
                metallic: 0.5,
                ..default()
            },
            extension: GridMaterial::new()
                .with_line_color(LinearRgba::new(0.1, 0.1, 0.1, 0.8))
                .with_grid_scale(0.5)
                .with_line_width(1.5)
                .with_major_line_every(4)
                .with_axes(GridAxes::ALL),
        })),
        Transform::from_xyz(-3.0, 1.5, 0.0),
        Rotating,
    ));

    // Sphere with XZ grid (shows how grid projects onto curved surfaces)
    commands.spawn((
        Mesh3d(meshes.add(Sphere::new(1.5).mesh().ico(5).unwrap())),
        MeshMaterial3d(materials.add(ExtendedMaterial {
            base: StandardMaterial {
                base_color: Color::srgb(0.2, 0.6, 0.8),
                perceptual_roughness: 0.2,
                metallic: 0.8,
                ..default()
            },
            extension: GridMaterial::new()
                .with_line_color(LinearRgba::new(0.0, 0.0, 0.0, 0.6))
                .with_grid_scale(1.0)
                .with_line_width(2.0)
                .with_major_line_every(10)
                .with_axes(GridAxes::ALL),
        })),
        Transform::from_xyz(3.0, 1.5, 0.0),
        Rotating,
    ));

    // Tall box with XY grid (vertical wall)
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::new(0.5, 4.0, 3.0))),
        MeshMaterial3d(materials.add(ExtendedMaterial {
            base: StandardMaterial {
                base_color: Color::srgb(0.3, 0.7, 0.3),
                perceptual_roughness: 0.5,
                ..default()
            },
            extension: GridMaterial::new()
                .with_line_color(LinearRgba::new(0.0, 0.3, 0.0, 0.7))
                .with_grid_scale(0.5)
                .with_major_line_every(4)
                .with_axes(GridAxes::XY | GridAxes::YZ),
        })),
        Transform::from_xyz(0.0, 2.0, -5.0),
    ));

    // Light
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -0.5, 0.5, 0.0)),
    ));

    // Ambient light
    commands.insert_resource(GlobalAmbientLight {
        color: Color::WHITE,
        brightness: 200.0,
        affects_lightmapped_meshes: true,
    });

    // Camera with fly controls
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(8.0, 6.0, 12.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
        FreeCamera {
            sensitivity: 0.15,
            walk_speed: 8.0,
            run_speed: 24.0,
            ..default()
        },
    ));
}

fn rotate_objects(time: Res<Time>, mut query: Query<&mut Transform, With<Rotating>>) {
    for mut transform in &mut query {
        transform.rotate_y(time.delta_secs() * 0.3);
    }
}
