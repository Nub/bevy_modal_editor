use avian3d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::SceneEntity;

/// Available primitive shapes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveShape {
    Cube,
    Sphere,
    Cylinder,
    Capsule,
    Plane,
}

impl PrimitiveShape {
    pub fn display_name(&self) -> &'static str {
        match self {
            PrimitiveShape::Cube => "Cube",
            PrimitiveShape::Sphere => "Sphere",
            PrimitiveShape::Cylinder => "Cylinder",
            PrimitiveShape::Capsule => "Capsule",
            PrimitiveShape::Plane => "Plane",
        }
    }
}

/// Event to spawn a primitive shape
#[derive(Message)]
pub struct SpawnPrimitiveEvent {
    pub shape: PrimitiveShape,
    pub position: Vec3,
}

/// Component to track what primitive shape an entity is
#[derive(Component, Serialize, Deserialize, Clone)]
pub struct PrimitiveMarker {
    pub shape: PrimitiveShape,
}

pub struct PrimitivesPlugin;

impl Plugin for PrimitivesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SpawnPrimitiveEvent>()
            .add_systems(Update, handle_spawn_primitive);
    }
}

fn handle_spawn_primitive(
    mut events: MessageReader<SpawnPrimitiveEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing_entities: Query<&Name, With<SceneEntity>>,
) {
    for event in events.read() {
        let name = generate_unique_name(event.shape.display_name(), &existing_entities);

        match event.shape {
            PrimitiveShape::Cube => {
                spawn_cube(&mut commands, &mut meshes, &mut materials, event.position, &name);
            }
            PrimitiveShape::Sphere => {
                spawn_sphere(&mut commands, &mut meshes, &mut materials, event.position, &name);
            }
            PrimitiveShape::Cylinder => {
                spawn_cylinder(&mut commands, &mut meshes, &mut materials, event.position, &name);
            }
            PrimitiveShape::Capsule => {
                spawn_capsule(&mut commands, &mut meshes, &mut materials, event.position, &name);
            }
            PrimitiveShape::Plane => {
                spawn_plane(&mut commands, &mut meshes, &mut materials, event.position, &name);
            }
        }
    }
}

fn generate_unique_name(base: &str, existing: &Query<&Name, With<SceneEntity>>) -> String {
    let mut counter = 1;
    loop {
        let name = format!("{} {}", base, counter);
        let exists = existing.iter().any(|n| n.as_str() == name);
        if !exists {
            return name;
        }
        counter += 1;
    }
}

fn spawn_cube(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    name: &str,
) {
    commands.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker {
            shape: PrimitiveShape::Cube,
        },
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.7, 0.6),
            ..default()
        })),
        Transform::from_translation(position),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
    ));
}

fn spawn_sphere(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    name: &str,
) {
    commands.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker {
            shape: PrimitiveShape::Sphere,
        },
        Mesh3d(meshes.add(Sphere::new(0.5))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.7, 0.8),
            ..default()
        })),
        Transform::from_translation(position),
        RigidBody::Static,
        Collider::sphere(0.5),
    ));
}

fn spawn_cylinder(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    name: &str,
) {
    commands.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker {
            shape: PrimitiveShape::Cylinder,
        },
        Mesh3d(meshes.add(Cylinder::new(0.5, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.7, 0.8, 0.6),
            ..default()
        })),
        Transform::from_translation(position),
        RigidBody::Static,
        Collider::cylinder(0.5, 0.5),
    ));
}

fn spawn_capsule(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    name: &str,
) {
    commands.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker {
            shape: PrimitiveShape::Capsule,
        },
        Mesh3d(meshes.add(Capsule3d::new(0.25, 0.5))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.6, 0.7),
            ..default()
        })),
        Transform::from_translation(position),
        RigidBody::Static,
        Collider::capsule(0.25, 0.5),
    ));
}

fn spawn_plane(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    name: &str,
) {
    commands.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker {
            shape: PrimitiveShape::Plane,
        },
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2.0, 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.6, 0.8),
            ..default()
        })),
        Transform::from_translation(position),
        RigidBody::Static,
        Collider::cuboid(2.0, 0.01, 2.0),
    ));
}
