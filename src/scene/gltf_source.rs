use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy::scene::SceneRoot;
use serde::{Deserialize, Serialize};

/// Component that specifies a GLTF/GLB file to load as children of this entity.
/// The path is relative to the assets folder.
#[derive(Component, Reflect, Default, Clone, Serialize, Deserialize)]
#[reflect(Component, Default)]
pub struct GltfSource {
    /// Path to the GLTF/GLB file (relative to assets folder)
    pub path: String,
    /// Which scene index to load (defaults to 0)
    pub scene_index: usize,
}

/// Marker component for the child entity that holds the loaded GLTF scene
#[derive(Component)]
pub struct GltfLoaded;

/// Tracks the currently loaded path to detect changes
#[derive(Component, Default)]
struct GltfLoadedPath {
    path: String,
    scene_index: usize,
}

pub struct GltfSourcePlugin;

impl Plugin for GltfSourcePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<GltfSource>()
            .add_systems(Update, (load_gltf_sources, cleanup_gltf_on_remove));
    }
}

/// System that loads GLTF scenes when GltfSource is added or changed
fn load_gltf_sources(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    sources: Query<(Entity, &GltfSource, Option<&GltfLoadedPath>), Changed<GltfSource>>,
    children_query: Query<&Children>,
    gltf_loaded_query: Query<Entity, With<GltfLoaded>>,
) {
    for (entity, source, loaded_path) in sources.iter() {
        // Check if the path actually changed
        if let Some(loaded) = loaded_path {
            if loaded.path == source.path && loaded.scene_index == source.scene_index {
                continue;
            }
        }

        // Remove any existing loaded GLTF children
        if let Ok(children) = children_query.get(entity) {
            for child in children.iter() {
                if gltf_loaded_query.get(child).is_ok() {
                    commands.entity(child).despawn();
                }
            }
        }

        // Load new GLTF if path is not empty
        if !source.path.is_empty() {
            let path = source.path.clone();
            let scene_index = source.scene_index;
            let scene_handle = asset_server.load(
                GltfAssetLabel::Scene(scene_index).from_asset(path),
            );

            // Spawn the scene as a child
            let child = commands
                .spawn((
                    GltfLoaded,
                    SceneRoot(scene_handle),
                    Transform::default(),
                ))
                .id();

            commands.entity(entity).add_child(child);

            // Track what we loaded
            commands.entity(entity).insert(GltfLoadedPath {
                path: source.path.clone(),
                scene_index: source.scene_index,
            });

            info!("Loading GLTF scene: {} (scene {})", source.path, source.scene_index);
        } else {
            // Remove the loaded path tracker if path is empty
            commands.entity(entity).remove::<GltfLoadedPath>();
        }
    }
}

/// Clean up GLTF children when the GltfSource component is removed
fn cleanup_gltf_on_remove(
    mut commands: Commands,
    mut removed: RemovedComponents<GltfSource>,
    children_query: Query<&Children>,
    gltf_loaded_query: Query<Entity, With<GltfLoaded>>,
) {
    for entity in removed.read() {
        // Remove any loaded GLTF children
        if let Ok(children) = children_query.get(entity) {
            for child in children.iter() {
                if gltf_loaded_query.get(child).is_ok() {
                    commands.entity(child).despawn();
                }
            }
        }

        // Remove the loaded path tracker
        if let Ok(mut entity_commands) = commands.get_entity(entity) {
            entity_commands.remove::<GltfLoadedPath>();
        }
    }
}
