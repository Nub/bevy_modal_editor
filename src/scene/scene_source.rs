use bevy::prelude::*;
use bevy::scene::serde::SceneDeserializer;
use serde::de::DeserializeSeed;
use serde::{Deserialize, Serialize};
use std::fs;

use super::{regenerate_runtime_components, GroupMarker, SceneEntity};

/// Component that specifies a RON scene file to load as children of this entity.
/// The scene entities will be loaded as children inside a group.
#[derive(Component, Reflect, Default, Clone, Serialize, Deserialize)]
#[reflect(Component, Default)]
pub struct SceneSource {
    /// Path to the RON scene file (absolute or relative to working directory)
    pub path: String,
}

/// Event to spawn a scene as a group in the editor
#[derive(Message)]
pub struct SpawnSceneSourceEvent {
    /// Path to the RON scene file
    pub path: String,
    /// Position to spawn at
    pub position: Vec3,
    /// Rotation to spawn with
    pub rotation: Quat,
}

/// Marker component for the child entities loaded from a SceneSource
#[derive(Component)]
pub struct SceneSourceLoaded;

/// Tracks the currently loaded path to detect changes
#[derive(Component, Default)]
struct SceneSourceLoadedPath {
    path: String,
}

pub struct SceneSourcePlugin;

impl Plugin for SceneSourcePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<SceneSource>()
            .add_message::<SpawnSceneSourceEvent>()
            .add_systems(Update, (load_scene_sources, cleanup_scene_on_remove, handle_spawn_scene_source));
    }
}

/// Handle spawning scene source objects
fn handle_spawn_scene_source(
    mut commands: Commands,
    mut events: MessageReader<SpawnSceneSourceEvent>,
) {
    for event in events.read() {
        // Extract filename for the entity name
        let name = event.path
            .rsplit('/')
            .next()
            .unwrap_or(&event.path)
            .trim_end_matches(".scn.ron")
            .trim_end_matches(".ron")
            .to_string();

        commands.spawn((
            SceneEntity,
            GroupMarker,
            Name::new(name),
            SceneSource {
                path: event.path.clone(),
            },
            Transform::from_translation(event.position).with_rotation(event.rotation),
        ));

        info!("Spawned scene source: {}", event.path);
    }
}

/// System that loads scenes when SceneSource is added or changed
fn load_scene_sources(
    mut commands: Commands,
    sources: Query<(Entity, &SceneSource, Option<&SceneSourceLoadedPath>), Changed<SceneSource>>,
    children_query: Query<&Children>,
    loaded_query: Query<Entity, With<SceneSourceLoaded>>,
) {
    for (entity, source, loaded_path) in sources.iter() {
        // Check if the path actually changed
        if let Some(loaded) = loaded_path {
            if loaded.path == source.path {
                continue;
            }
        }

        // Remove any existing loaded scene children
        if let Ok(children) = children_query.get(entity) {
            for child in children.iter() {
                if loaded_query.get(child).is_ok() {
                    commands.entity(child).despawn();
                }
            }
        }

        // Load new scene if path is not empty
        if !source.path.is_empty() {
            // Queue the scene loading command
            commands.queue(LoadSceneSourceCommand {
                parent_entity: entity,
                path: source.path.clone(),
            });
        } else {
            // Remove the loaded path tracker if path is empty
            commands.entity(entity).remove::<SceneSourceLoadedPath>();
        }
    }
}

/// Command to load a scene source with exclusive world access
struct LoadSceneSourceCommand {
    parent_entity: Entity,
    path: String,
}

impl Command for LoadSceneSourceCommand {
    fn apply(self, world: &mut World) {
        // Read scene file
        let content = match fs::read_to_string(&self.path) {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to read scene source file '{}': {}", self.path, e);
                return;
            }
        };

        // Deserialize scene
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry_guard = type_registry.read();
        let scene_deserializer = SceneDeserializer {
            type_registry: &type_registry_guard,
        };

        let mut ron_deserializer = match ron::de::Deserializer::from_str(&content) {
            Ok(d) => d,
            Err(e) => {
                error!("Failed to parse scene source file '{}': {}", self.path, e);
                return;
            }
        };

        let scene: DynamicScene = match scene_deserializer.deserialize(&mut ron_deserializer) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to deserialize scene source '{}': {:?}", self.path, e);
                return;
            }
        };

        // Drop the type registry borrow before writing to world
        drop(type_registry_guard);

        // Write scene to world
        let mut entity_map = bevy::ecs::entity::EntityHashMap::default();
        if let Err(e) = scene.write_to_world(world, &mut entity_map) {
            error!("Failed to instantiate scene source '{}': {:?}", self.path, e);
            return;
        }

        // Get root entities from the loaded scene (entities that were in the scene file)
        // These are the ones that got mapped and don't have parents in the map
        let loaded_entities: Vec<Entity> = entity_map.values().copied().collect();

        // Find root entities (those without parents that were also loaded)
        let root_entities: Vec<Entity> = loaded_entities
            .iter()
            .filter(|&&entity| {
                // Check if this entity has a parent that was also loaded
                if let Ok(entity_ref) = world.get_entity(entity) {
                    if let Some(child_of) = entity_ref.get::<ChildOf>() {
                        // Parent exists in the loaded set means it's not a root
                        !loaded_entities.contains(&child_of.parent())
                    } else {
                        true // No parent = root
                    }
                } else {
                    false
                }
            })
            .copied()
            .collect();

        // Parent the root entities to our parent entity and mark them
        for loaded_entity in root_entities {
            // Mark as loaded from this scene source
            if let Ok(mut entity_mut) = world.get_entity_mut(loaded_entity) {
                entity_mut.insert(SceneSourceLoaded);
            }

            // Parent to our entity
            if let Ok(mut parent_entity) = world.get_entity_mut(self.parent_entity) {
                parent_entity.add_child(loaded_entity);
            }
        }

        // Track what we loaded
        if let Ok(mut parent_entity) = world.get_entity_mut(self.parent_entity) {
            parent_entity.insert(SceneSourceLoadedPath {
                path: self.path.clone(),
            });
        }

        // Regenerate runtime components for loaded primitives, lights, etc.
        regenerate_runtime_components(world);

        info!("Loaded scene source: {} ({} entities)", self.path, loaded_entities.len());
    }
}

/// Clean up scene children when the SceneSource component is removed
fn cleanup_scene_on_remove(
    mut commands: Commands,
    mut removed: RemovedComponents<SceneSource>,
    children_query: Query<&Children>,
    loaded_query: Query<Entity, With<SceneSourceLoaded>>,
) {
    for entity in removed.read() {
        // Remove any loaded scene children
        if let Ok(children) = children_query.get(entity) {
            for child in children.iter() {
                if loaded_query.get(child).is_ok() {
                    commands.entity(child).despawn();
                }
            }
        }

        // Remove the loaded path tracker
        if let Ok(mut entity_commands) = commands.get_entity(entity) {
            entity_commands.remove::<SceneSourceLoadedPath>();
        }
    }
}
