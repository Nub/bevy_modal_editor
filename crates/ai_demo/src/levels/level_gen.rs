//! Level file generation and serializable entity spawn helpers.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_editor_game::{BaseMaterialProps, MaterialDefinition, MaterialRef};
use bevy_modal_editor::scene::build_editor_scene;
use bevy_modal_editor::SceneEntity;
use std::fs;
use std::path::Path;

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

/// Exclusive startup system: generate missing level file and auto-load it.
pub fn generate_and_load_level(world: &mut World) {
    let _ = fs::create_dir_all(LEVELS_DIR);

    let filename = "arena.scn.ron";
    let path = level_path(filename);

    if !Path::new(&path).exists() {
        info!("Generating arena level: {}", path);

        super::arena::build_arena(world);

        let entity_ids: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
            query.iter(world).collect()
        };

        if entity_ids.is_empty() {
            warn!("Arena builder produced no SceneEntity entities");
            return;
        }

        let scene = build_editor_scene(world, entity_ids.iter().copied());

        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();
        match scene.serialize(&type_registry) {
            Ok(ron_data) => {
                if let Err(e) = fs::write(&path, &ron_data) {
                    error!("Failed to write level file: {}", e);
                }
            }
            Err(e) => {
                error!("Failed to serialize arena: {:?}", e);
            }
        }
        drop(type_registry);

        for entity in entity_ids {
            world.despawn(entity);
        }

        info!("Generated arena level file");
    }
}
