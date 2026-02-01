//! Example showing how to add the editor to an existing Bevy scene.
//!
//! Run with: `cargo run --example with_existing_scene`
//!
//! This demonstrates:
//! - Adding the editor plugin to a game with existing entities
//! - Marking entities as editable with `SceneEntity`
//! - Using physics components that the editor understands

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_avian3d_editor::{EditorPlugin, SceneEntity};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy Avian3D Editor - Existing Scene Example".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EditorPlugin::default())
        .add_systems(Startup, setup_scene)
        .run();
}

/// Setup an existing game scene with some objects
fn setup_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Ground plane - static physics body
    commands.spawn((
        Name::new("Ground"),
        SceneEntity, // Mark as editable by the editor
        Mesh3d(meshes.add(Plane3d::new(Vec3::Y, Vec2::splat(50.0)))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.5, 0.3),
            ..default()
        })),
        Transform::from_xyz(0.0, 0.0, 0.0),
        RigidBody::Static,
        Collider::half_space(Vec3::Y),
    ));

    // A few cubes with physics
    let cube_mesh = meshes.add(Cuboid::new(1.0, 1.0, 1.0));
    let cube_material = materials.add(StandardMaterial {
        base_color: Color::srgb(0.8, 0.2, 0.2),
        ..default()
    });

    for i in 0..5 {
        commands.spawn((
            Name::new(format!("Cube {}", i + 1)),
            SceneEntity,
            Mesh3d(cube_mesh.clone()),
            MeshMaterial3d(cube_material.clone()),
            Transform::from_xyz(i as f32 * 2.0 - 4.0, 0.5, 0.0),
            RigidBody::Dynamic,
            Collider::cuboid(1.0, 1.0, 1.0),
        ));
    }

    // A sphere
    commands.spawn((
        Name::new("Sphere"),
        SceneEntity,
        Mesh3d(meshes.add(Sphere::new(0.5))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.2, 0.8),
            ..default()
        })),
        Transform::from_xyz(0.0, 3.0, 0.0),
        RigidBody::Dynamic,
        Collider::sphere(0.5),
    ));

    // A cylinder
    commands.spawn((
        Name::new("Cylinder"),
        SceneEntity,
        Mesh3d(meshes.add(Cylinder::new(0.5, 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.2, 0.8, 0.2),
            ..default()
        })),
        Transform::from_xyz(3.0, 1.0, 3.0),
        RigidBody::Dynamic,
        Collider::cylinder(0.5, 2.0),
    ));
}
