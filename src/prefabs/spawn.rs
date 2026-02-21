use bevy::prelude::*;
use bevy::scene::serde::SceneDeserializer;
use serde::de::DeserializeSeed;
use std::fs;

use super::prefab::{PrefabInstance, PrefabRoot};
use super::registry::PrefabRegistry;
use crate::scene::{regenerate_runtime_components, GroupMarker, SceneEntity};

/// Event to spawn a prefab instance into the scene
#[derive(Message)]
pub struct SpawnPrefabEvent {
    pub prefab_name: String,
    pub position: Vec3,
    pub rotation: Quat,
}

/// Event to create a prefab from selected entities
#[derive(Message)]
pub struct CreatePrefabEvent {
    pub name: String,
    pub entities: Vec<Entity>,
}

/// Event to open a prefab for editing in a separate context
#[derive(Message)]
pub struct OpenPrefabEvent {
    pub prefab_name: String,
}

/// Event to close the current prefab editing context
#[derive(Message)]
pub struct ClosePrefabEvent;

/// Command that loads a prefab scene into the world additively
struct SpawnPrefabCommand {
    prefab_name: String,
    scene_content: String,
    position: Vec3,
    rotation: Quat,
    instance_id: String,
}

impl Command for SpawnPrefabCommand {
    fn apply(self, world: &mut World) {
        // Deserialize the prefab scene
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();
        let scene_deserializer = SceneDeserializer {
            type_registry: &type_registry,
        };

        let mut ron_deserializer = match ron::de::Deserializer::from_str(&self.scene_content) {
            Ok(d) => d,
            Err(e) => {
                error!("Failed to parse prefab '{}': {}", self.prefab_name, e);
                return;
            }
        };

        let scene: DynamicScene = match scene_deserializer.deserialize(&mut ron_deserializer) {
            Ok(s) => s,
            Err(e) => {
                error!(
                    "Failed to deserialize prefab '{}': {:?}",
                    self.prefab_name, e
                );
                return;
            }
        };

        drop(type_registry);

        // Write scene entities into the world (additive — doesn't clear existing entities)
        let mut entity_map = bevy::ecs::entity::EntityHashMap::default();
        if let Err(e) = scene.write_to_world(world, &mut entity_map) {
            error!(
                "Failed to instantiate prefab '{}': {:?}",
                self.prefab_name, e
            );
            return;
        }

        // Collect the new entities that were created
        let new_entities: Vec<Entity> = entity_map.values().copied().collect();

        // Tag all new entities with PrefabInstance
        for &entity in &new_entities {
            world.entity_mut(entity).insert(PrefabInstance {
                prefab_name: self.prefab_name.clone(),
                instance_id: self.instance_id.clone(),
            });
        }

        // Find root entities (those without a parent among the new entities)
        let root_entities: Vec<Entity> = new_entities
            .iter()
            .filter(|&&entity| {
                let parent = world.entity(entity).get::<ChildOf>();
                match parent {
                    Some(child_of) => !new_entities.contains(&child_of.parent()),
                    None => true,
                }
            })
            .copied()
            .collect();

        // Create a group container for the prefab instance
        let display_name = format!("[Prefab] {}", self.instance_id);
        let group_entity = world
            .spawn((
                SceneEntity,
                GroupMarker,
                PrefabRoot,
                PrefabInstance {
                    prefab_name: self.prefab_name.clone(),
                    instance_id: self.instance_id.clone(),
                },
                Name::new(display_name),
                Transform::from_translation(self.position)
                    .with_rotation(self.rotation),
            ))
            .id();

        // Reparent root entities under the group and zero out their world offset
        // (since the group provides the position)
        for &root in &root_entities {
            world.entity_mut(root).insert(ChildOf(group_entity));
        }

        // Regenerate runtime components (meshes, materials, colliders, lights)
        regenerate_runtime_components(world);
        crate::scene::resolve_entity_references(world);

        info!(
            "Spawned prefab '{}' as instance '{}' ({} entities)",
            self.prefab_name,
            self.instance_id,
            new_entities.len()
        );
    }
}

pub fn handle_spawn_prefab(
    mut events: MessageReader<SpawnPrefabEvent>,
    mut commands: Commands,
    mut registry: ResMut<PrefabRegistry>,
) {
    for event in events.read() {
        let Some(entry) = registry.get(&event.prefab_name) else {
            warn!("Prefab not found: {}", event.prefab_name);
            continue;
        };

        // Read the prefab scene file
        let content = match fs::read_to_string(&entry.scene_path) {
            Ok(c) => c,
            Err(e) => {
                error!(
                    "Failed to read prefab scene '{}': {}",
                    event.prefab_name, e
                );
                continue;
            }
        };

        let instance_id = registry.next_instance_id(&event.prefab_name);

        commands.queue(SpawnPrefabCommand {
            prefab_name: event.prefab_name.clone(),
            scene_content: content,
            position: event.position,
            rotation: event.rotation,
            instance_id,
        });

        info!("Queued prefab spawn: {}", event.prefab_name);
    }
}
