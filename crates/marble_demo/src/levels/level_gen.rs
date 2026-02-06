//! Level file generation and serializable entity spawn helpers.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_editor_game::{BaseMaterialProps, MaterialDefinition, MaterialRef};
use bevy_modal_editor::scene::build_editor_scene;
use bevy_modal_editor::SceneEntity;
use std::fs;
use std::path::Path;

use super::LevelRegistry;

/// Base directory for level files (resolved at compile time).
const LEVELS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/levels/");

/// Get the full path for a level filename.
pub fn level_path(filename: &str) -> String {
    format!("{}{}", LEVELS_DIR, filename)
}

/// Spawn a serializable cube entity (no Mesh3d, Collider, or material handles).
/// These are regenerated from PrimitiveMarker + MaterialRef on scene load.
pub fn spawn_cube(
    world: &mut World,
    name: &str,
    transform: Transform,
    albedo: &str,
    normal: &str,
    uv_scale: [f32; 2],
    base_color: Color,
) {
    world.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        bevy_modal_editor::PrimitiveMarker {
            shape: bevy_modal_editor::PrimitiveShape::Cube,
        },
        MaterialRef::Inline(MaterialDefinition {
            base: BaseMaterialProps {
                base_color,
                base_color_texture: Some(albedo.into()),
                normal_map_texture: Some(normal.into()),
                uv_scale,
                ..default()
            },
            extension: None,
        }),
        transform,
        RigidBody::Static,
    ));
}

/// Spawn a serializable cylinder entity (no Mesh3d, Collider, or material handles).
pub fn spawn_cylinder(
    world: &mut World,
    name: &str,
    transform: Transform,
    albedo: &str,
    normal: &str,
    uv_scale: [f32; 2],
    base_color: Color,
) {
    world.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        bevy_modal_editor::PrimitiveMarker {
            shape: bevy_modal_editor::PrimitiveShape::Cylinder,
        },
        MaterialRef::Inline(MaterialDefinition {
            base: BaseMaterialProps {
                base_color,
                base_color_texture: Some(albedo.into()),
                normal_map_texture: Some(normal.into()),
                uv_scale,
                ..default()
            },
            extension: None,
        }),
        transform,
        RigidBody::Static,
    ));
}

/// Exclusive startup system: generate missing `.scn.ron` level files.
///
/// For each registered level whose file doesn't exist on disk, this system:
/// 1. Runs the builder function to spawn serializable-only entities
/// 2. Builds a DynamicScene from those entities
/// 3. Serializes to RON and writes to `assets/levels/<filename>`
/// 4. Despawns the temporary entities
pub fn generate_missing_level_files(world: &mut World) {
    // Ensure the levels directory exists
    let _ = fs::create_dir_all(LEVELS_DIR);

    let registry = world.resource::<LevelRegistry>().clone();

    for level in &registry.levels {
        let path = level_path(&level.filename);
        if Path::new(&path).exists() {
            info!("Level file exists: {}", path);
            continue;
        }

        info!("Generating level file: {} ({})", level.name, path);

        // Run the builder to spawn serializable entities
        (level.builder)(world);

        // Collect all SceneEntity entities
        let entity_ids: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
            query.iter(world).collect()
        };

        if entity_ids.is_empty() {
            warn!("Builder for '{}' produced no SceneEntity entities", level.name);
            continue;
        }

        // Build scene using the editor's standard allow-list
        let scene = build_editor_scene(world, entity_ids.iter().copied());

        // Serialize
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();
        match scene.serialize(&type_registry) {
            Ok(ron_data) => {
                if let Err(e) = fs::write(&path, &ron_data) {
                    error!("Failed to write level file '{}': {}", path, e);
                }
            }
            Err(e) => {
                error!("Failed to serialize level '{}': {:?}", level.name, e);
            }
        }
        drop(type_registry);

        // Despawn the temporary entities
        for entity in entity_ids {
            world.despawn(entity);
        }

        info!("Generated level file: {}", path);
    }
}
