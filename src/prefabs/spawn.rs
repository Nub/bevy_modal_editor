use avian3d::prelude::*;
use bevy::prelude::*;

use super::prefab::{Prefab, PrefabEntity, PrefabInstance, PrefabRoot};
use super::registry::PrefabRegistry;
use crate::scene::{PrimitiveMarker, PrimitiveShape, SceneEntity, SerializedRigidBody};

/// Event to spawn a prefab instance
#[derive(Message)]
pub struct SpawnPrefabEvent {
    pub prefab_name: String,
    pub position: Vec3,
}

/// Event to create a prefab from selected entities
#[derive(Message)]
pub struct CreatePrefabEvent {
    pub name: String,
    pub entities: Vec<Entity>,
}

pub struct PrefabSpawnPlugin;

impl Plugin for PrefabSpawnPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SpawnPrefabEvent>()
            .add_message::<CreatePrefabEvent>()
            .add_systems(Update, (handle_spawn_prefab, handle_create_prefab));
    }
}

fn handle_spawn_prefab(
    mut events: MessageReader<SpawnPrefabEvent>,
    registry: Res<PrefabRegistry>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for event in events.read() {
        let Some(prefab) = registry.get(&event.prefab_name) else {
            warn!("Prefab not found: {}", event.prefab_name);
            continue;
        };

        spawn_prefab_entities(
            &mut commands,
            &mut meshes,
            &mut materials,
            prefab,
            event.position,
        );

        info!("Spawned prefab: {}", event.prefab_name);
    }
}

fn spawn_prefab_entities(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    prefab: &Prefab,
    offset: Vec3,
) {
    for entity_data in &prefab.entities {
        spawn_prefab_entity_recursive(commands, meshes, materials, entity_data, offset, true, &prefab.name);
    }
}

fn spawn_prefab_entity_recursive(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    entity_data: &PrefabEntity,
    offset: Vec3,
    is_root: bool,
    prefab_name: &str,
) {
    let mut transform: Transform = (&entity_data.transform).into();
    if is_root {
        transform.translation += offset;
    }

    let mut entity_commands = commands.spawn((
        SceneEntity,
        Name::new(entity_data.name.clone()),
        transform,
        PrefabInstance {
            prefab_name: prefab_name.to_string(),
        },
    ));

    if is_root {
        entity_commands.insert(PrefabRoot);
    }

    // Add primitive mesh and collider
    if let Some(shape) = entity_data.primitive {
        entity_commands.insert(PrimitiveMarker { shape });
        add_primitive_components(&mut entity_commands, meshes, materials, shape);
    }

    // Add rigid body
    if let Some(ref rb) = entity_data.rigid_body {
        entity_commands.insert(RigidBody::from(rb));
    }

    // Spawn children (for now, spawn as separate entities - proper hierarchy coming later)
    for child in &entity_data.children {
        spawn_prefab_entity_recursive(commands, meshes, materials, child, offset, false, prefab_name);
    }
}

fn add_primitive_components(
    entity_commands: &mut bevy::ecs::system::EntityCommands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    shape: PrimitiveShape,
) {
    match shape {
        PrimitiveShape::Cube => {
            entity_commands.insert((
                Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.8, 0.7, 0.6),
                    ..default()
                })),
                Collider::cuboid(1.0, 1.0, 1.0),
            ));
        }
        PrimitiveShape::Sphere => {
            entity_commands.insert((
                Mesh3d(meshes.add(Sphere::new(0.5))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.6, 0.7, 0.8),
                    ..default()
                })),
                Collider::sphere(0.5),
            ));
        }
        PrimitiveShape::Cylinder => {
            entity_commands.insert((
                Mesh3d(meshes.add(Cylinder::new(0.5, 1.0))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.7, 0.8, 0.6),
                    ..default()
                })),
                Collider::cylinder(0.5, 0.5),
            ));
        }
        PrimitiveShape::Capsule => {
            entity_commands.insert((
                Mesh3d(meshes.add(Capsule3d::new(0.25, 0.5))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.8, 0.6, 0.7),
                    ..default()
                })),
                Collider::capsule(0.25, 0.5),
            ));
        }
        PrimitiveShape::Plane => {
            entity_commands.insert((
                Mesh3d(meshes.add(Plane3d::default().mesh().size(2.0, 2.0))),
                MeshMaterial3d(materials.add(StandardMaterial {
                    base_color: Color::srgb(0.6, 0.6, 0.8),
                    ..default()
                })),
                Collider::cuboid(2.0, 0.01, 2.0),
            ));
        }
    }
}

fn handle_create_prefab(
    mut events: MessageReader<CreatePrefabEvent>,
    mut registry: ResMut<PrefabRegistry>,
    entities: Query<(
        &Name,
        &Transform,
        Option<&PrimitiveMarker>,
        Option<&RigidBody>,
    )>,
) {
    for event in events.read() {
        let mut prefab = Prefab {
            name: event.name.clone(),
            entities: Vec::new(),
        };

        for entity in &event.entities {
            if let Ok((name, transform, primitive, rigid_body)) = entities.get(*entity) {
                prefab.entities.push(PrefabEntity {
                    name: name.as_str().to_string(),
                    transform: transform.into(),
                    primitive: primitive.map(|p| p.shape),
                    rigid_body: rigid_body.map(SerializedRigidBody::from),
                    children: Vec::new(),
                });
            }
        }

        if let Err(e) = registry.save_prefab(&prefab) {
            error!("Failed to save prefab: {}", e);
        } else {
            registry.prefabs.insert(prefab.name.clone(), prefab);
        }
    }
}
