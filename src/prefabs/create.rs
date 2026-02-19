use bevy::prelude::*;
use bevy_editor_game::MaterialLibrary;
use std::collections::HashMap;
use std::fs;

use super::assets::{bundle_assets_into_prefab, collect_asset_paths, remap_material_library};
use super::registry::PrefabRegistry;
use super::spawn::{CreatePrefabEvent, SpawnPrefabEvent};
use crate::scene::{build_editor_scene, SceneEntity};
use crate::selection::Selected;

/// Command to create a prefab from selected entities (needs exclusive world access)
struct CreatePrefabCommand {
    name: String,
    entities: Vec<Entity>,
}

impl Command for CreatePrefabCommand {
    fn apply(self, world: &mut World) {
        let registry = world.resource::<PrefabRegistry>();
        let prefab_dir = registry.root_directory.join(&self.name);

        // Create the prefab directory
        if let Err(e) = fs::create_dir_all(&prefab_dir) {
            error!("Failed to create prefab directory: {}", e);
            return;
        }

        // Collect only selected entities that are SceneEntities
        let valid_entities: Vec<Entity> = self
            .entities
            .iter()
            .filter(|&&e| world.entity(e).contains::<SceneEntity>())
            .copied()
            .collect();

        if valid_entities.is_empty() {
            warn!("No valid scene entities selected for prefab creation");
            return;
        }

        // Compute center position of the selected entities for spawning the replacement
        let center_position = {
            let mut sum = Vec3::ZERO;
            let mut count = 0;
            for &entity in &valid_entities {
                if let Some(transform) = world.entity(entity).get::<Transform>() {
                    sum += transform.translation;
                    count += 1;
                }
            }
            if count > 0 {
                sum / count as f32
            } else {
                Vec3::ZERO
            }
        };

        // Get material library for asset path collection
        let material_library = world
            .get_resource::<MaterialLibrary>()
            .cloned()
            .unwrap_or_default();

        // Collect and bundle assets
        let asset_paths = collect_asset_paths(world, &valid_entities, &material_library);
        let remap = bundle_assets_into_prefab(&asset_paths, &prefab_dir);

        // Build a material library subset with only referenced materials
        let mut prefab_materials = collect_referenced_materials(world, &valid_entities, &material_library);
        remap_material_library(&mut prefab_materials, &remap);

        // Build scene from the selected entities
        let scene = build_editor_scene(world, valid_entities.into_iter());

        // Serialize
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();

        match scene.serialize(&type_registry) {
            Ok(serialized) => {
                let scene_path = prefab_dir.join(format!("{}.scn.ron", self.name));
                if let Err(e) = fs::write(&scene_path, &serialized) {
                    error!("Failed to write prefab scene: {}", e);
                    return;
                }

                // Save metadata sidecar
                if let Err(e) = PrefabRegistry::save_metadata(
                    &prefab_dir,
                    &self.name,
                    &prefab_materials,
                    &HashMap::new(),
                ) {
                    error!("Failed to write prefab metadata: {}", e);
                    return;
                }

                info!("Created prefab '{}' at {:?}", self.name, prefab_dir);
            }
            Err(e) => {
                error!("Failed to serialize prefab scene: {:?}", e);
                return;
            }
        }

        drop(type_registry);

        // Refresh the registry to pick up the new prefab
        if let Some(mut registry) = world.get_resource_mut::<PrefabRegistry>() {
            registry.refresh();
        }

        // Despawn the original entities and replace with a prefab instance
        for &entity in &self.entities {
            if let Ok(mut e) = world.get_entity_mut(entity) {
                e.remove::<Selected>();
            }
            world.despawn(entity);
        }

        // Spawn the newly created prefab in place of the originals
        world.write_message(SpawnPrefabEvent {
            prefab_name: self.name.clone(),
            position: center_position,
            rotation: Quat::IDENTITY,
        });
    }
}

/// Collect materials referenced by the given entities into a subset library.
fn collect_referenced_materials(
    world: &World,
    entities: &[Entity],
    full_library: &MaterialLibrary,
) -> MaterialLibrary {
    use bevy_editor_game::MaterialRef;

    let mut subset = MaterialLibrary::default();

    for &entity in entities {
        if let Some(mat_ref) = world.entity(entity).get::<MaterialRef>() {
            match mat_ref {
                MaterialRef::Library(name) => {
                    if let Some(def) = full_library.materials.get(name) {
                        subset.materials.insert(name.clone(), def.clone());
                    }
                }
                MaterialRef::Inline(_) => {
                    // Inline materials are stored on the entity, not in the library
                }
            }
        }
    }

    subset
}

pub fn handle_create_prefab(
    mut events: MessageReader<CreatePrefabEvent>,
    mut commands: Commands,
    selected: Query<Entity, With<Selected>>,
) {
    for event in events.read() {
        let entities = if event.entities.is_empty() {
            // If no entities specified, use current selection
            selected.iter().collect()
        } else {
            event.entities.clone()
        };

        if entities.is_empty() {
            warn!("No entities to create prefab from");
            continue;
        }

        commands.queue(CreatePrefabCommand {
            name: event.name.clone(),
            entities,
        });
    }
}
