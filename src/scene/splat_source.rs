use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_gaussian_splatting::{CloudSettings, GaussianCamera, PlanarGaussian3dHandle};
use serde::{Deserialize, Serialize};

use crate::editor::EditorCamera;
use super::SceneEntity;

/// Component that specifies a gaussian splat file to load as a child of this entity.
/// The path is relative to the assets folder.
#[derive(Component, Reflect, Default, Clone, Serialize, Deserialize)]
#[reflect(Component, Default)]
pub struct SplatSource {
    /// Path to the .ply/.splat/.gcloud file (relative to assets folder)
    pub path: String,
}

/// Event to spawn a gaussian splat object in the scene
#[derive(Message)]
pub struct SpawnSplatEvent {
    /// Path to the splat file (relative to assets folder)
    pub path: String,
    /// Position to spawn at
    pub position: Vec3,
    /// Rotation to spawn with
    pub rotation: Quat,
}

/// Marker component for the child entity that holds the loaded splat
#[derive(Component)]
pub struct SplatLoaded;

/// Tracks the currently loaded path to detect changes
#[derive(Component, Default)]
struct SplatLoadedPath {
    path: String,
}

pub struct SplatSourcePlugin;

impl Plugin for SplatSourcePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<SplatSource>()
            .add_message::<SpawnSplatEvent>()
            .add_systems(
                Update,
                (load_splat_sources, cleanup_splat_on_remove, handle_spawn_splat, sync_gaussian_camera, sync_cloud_settings),
            );
    }
}

/// Handle spawning splat objects
fn handle_spawn_splat(mut commands: Commands, mut events: MessageReader<SpawnSplatEvent>) {
    for event in events.read() {
        // Extract filename for the entity name
        let name = event
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&event.path)
            .trim_end_matches(".ply")
            .trim_end_matches(".splat")
            .trim_end_matches(".gcloud")
            .to_string();

        commands.spawn((
            SceneEntity,
            Name::new(name),
            SplatSource {
                path: event.path.clone(),
            },
            CloudSettings::default(),
            Transform::from_translation(event.position).with_rotation(event.rotation),
            RigidBody::Static,
            Collider::sphere(1.0),
        ));

        info!("Spawned gaussian splat: {}", event.path);
    }
}

/// System that loads splat files when SplatSource is added or changed
fn load_splat_sources(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    sources: Query<(Entity, &SplatSource, Option<&SplatLoadedPath>), Changed<SplatSource>>,
    children_query: Query<&Children>,
    splat_loaded_query: Query<Entity, With<SplatLoaded>>,
) {
    for (entity, source, loaded_path) in sources.iter() {
        // Check if the path actually changed
        if let Some(loaded) = loaded_path {
            if loaded.path == source.path {
                continue;
            }
        }

        // Remove any existing loaded splat children
        if let Ok(children) = children_query.get(entity) {
            for child in children.iter() {
                if splat_loaded_query.get(child).is_ok() {
                    commands.entity(child).despawn();
                }
            }
        }

        // Load new splat if path is not empty
        if !source.path.is_empty() {
            let handle: Handle<bevy_gaussian_splatting::PlanarGaussian3d> =
                asset_server.load(source.path.clone());

            // Spawn the splat as a child
            let child = commands
                .spawn((
                    SplatLoaded,
                    PlanarGaussian3dHandle(handle),
                    CloudSettings::default(),
                    Transform::default(),
                ))
                .id();

            commands.entity(entity).add_child(child);

            // Track what we loaded
            commands.entity(entity).insert(SplatLoadedPath {
                path: source.path.clone(),
            });

            info!("Loading gaussian splat: {}", source.path);
        } else {
            // Remove the loaded path tracker if path is empty
            commands.entity(entity).remove::<SplatLoadedPath>();
        }
    }
}

/// Clean up splat children when the SplatSource component is removed
fn cleanup_splat_on_remove(
    mut commands: Commands,
    mut removed: RemovedComponents<SplatSource>,
    children_query: Query<&Children>,
    splat_loaded_query: Query<Entity, With<SplatLoaded>>,
) {
    for entity in removed.read() {
        // Remove any loaded splat children
        if let Ok(children) = children_query.get(entity) {
            for child in children.iter() {
                if splat_loaded_query.get(child).is_ok() {
                    commands.entity(child).despawn();
                }
            }
        }

        // Remove the loaded path tracker
        if let Ok(mut entity_commands) = commands.get_entity(entity) {
            entity_commands.remove::<SplatLoadedPath>();
        }
    }
}

/// Sync CloudSettings from parent (SceneEntity) to child (SplatLoaded) when changed.
fn sync_cloud_settings(
    parents: Query<(&CloudSettings, &Children), (With<SplatSource>, Changed<CloudSettings>)>,
    mut children: Query<&mut CloudSettings, (With<SplatLoaded>, Without<SplatSource>)>,
) {
    for (parent_settings, parent_children) in parents.iter() {
        for child in parent_children.iter() {
            if let Ok(mut child_settings) = children.get_mut(child) {
                *child_settings = parent_settings.clone();
            }
        }
    }
}

/// Add/remove `GaussianCamera` on the editor camera based on whether any splats exist.
fn sync_gaussian_camera(
    mut commands: Commands,
    splat_sources: Query<(), With<SplatSource>>,
    camera_with: Query<Entity, (With<EditorCamera>, With<GaussianCamera>)>,
    camera_without: Query<Entity, (With<EditorCamera>, Without<GaussianCamera>)>,
) {
    let has_splats = !splat_sources.is_empty();
    if has_splats {
        if let Ok(entity) = camera_without.single() {
            commands
                .entity(entity)
                .insert(GaussianCamera { warmup: false });
        }
    } else if let Ok(entity) = camera_with.single() {
        commands.entity(entity).remove::<GaussianCamera>();
    }
}
