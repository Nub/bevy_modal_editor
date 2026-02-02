use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::scene::serde::SceneDeserializer;
use bevy::window::PrimaryWindow;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use serde::de::DeserializeSeed;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::{DirectionalLightMarker, GltfSource, GroupMarker, Locked, PrimitiveMarker, PrimitiveShape, RecursiveColliderConstructor, SceneEntity, SceneLightMarker, SceneSource, LIGHT_COLLIDER_RADIUS};
use crate::editor::{CameraMark, CameraMarks};
use crate::ui::draw_error_dialog as draw_themed_error_dialog;

/// Event to save the scene
#[derive(Message)]
pub struct SaveSceneEvent {
    pub path: String,
}

/// Event to load a scene
#[derive(Message)]
pub struct LoadSceneEvent {
    pub path: String,
}

/// Event to force save (skip overwrite check)
#[derive(Message)]
pub struct ForceSaveSceneEvent {
    pub path: String,
}

/// Resource to store scene loading/saving errors for display
#[derive(Resource, Default)]
pub struct SceneErrorDialog {
    pub open: bool,
    pub title: String,
    pub message: String,
}

/// Resource for overwrite confirmation dialog
#[derive(Resource, Default)]
pub struct OverwriteConfirmDialog {
    pub open: bool,
    pub path: String,
}

/// Resource to track the current scene file and modification state
#[derive(Resource, Default)]
pub struct SceneFile {
    /// Path to the current scene file (None if untitled/new)
    pub path: Option<String>,
    /// Whether the scene has unsaved modifications
    pub modified: bool,
}

impl SceneFile {
    /// Get the display name for the current file
    pub fn display_name(&self) -> &str {
        self.path
            .as_ref()
            .and_then(|p| Path::new(p).file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled")
    }

    /// Mark the scene as modified
    pub fn mark_modified(&mut self) {
        self.modified = true;
    }

    /// Clear the modified flag (called after save)
    pub fn clear_modified(&mut self) {
        self.modified = false;
    }
}

/// Sidecar data for editor-specific metadata (camera marks, etc.)
#[derive(Serialize, Deserialize, Default)]
struct EditorMetadata {
    camera_marks: HashMap<String, CameraMark>,
}

/// Serializable transform data (used by prefabs)
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct SerializedTransform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl From<&Transform> for SerializedTransform {
    fn from(t: &Transform) -> Self {
        Self {
            translation: t.translation.into(),
            rotation: t.rotation.into(),
            scale: t.scale.into(),
        }
    }
}

impl From<&SerializedTransform> for Transform {
    fn from(s: &SerializedTransform) -> Self {
        Transform {
            translation: s.translation.into(),
            rotation: Quat::from_array(s.rotation),
            scale: s.scale.into(),
        }
    }
}

/// Serializable rigid body type (used by prefabs)
#[derive(Serialize, Deserialize, Clone, Default)]
pub enum SerializedRigidBody {
    #[default]
    Static,
    Dynamic,
    Kinematic,
}

impl From<&RigidBody> for SerializedRigidBody {
    fn from(rb: &RigidBody) -> Self {
        match rb {
            RigidBody::Static => SerializedRigidBody::Static,
            RigidBody::Dynamic => SerializedRigidBody::Dynamic,
            RigidBody::Kinematic => SerializedRigidBody::Kinematic,
        }
    }
}

impl From<&SerializedRigidBody> for RigidBody {
    fn from(s: &SerializedRigidBody) -> Self {
        match s {
            SerializedRigidBody::Static => RigidBody::Static,
            SerializedRigidBody::Dynamic => RigidBody::Dynamic,
            SerializedRigidBody::Kinematic => RigidBody::Kinematic,
        }
    }
}

pub struct SerializationPlugin;

impl Plugin for SerializationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SceneErrorDialog>()
            .init_resource::<SceneFile>()
            .init_resource::<OverwriteConfirmDialog>()
            .add_message::<SaveSceneEvent>()
            .add_message::<LoadSceneEvent>()
            .add_message::<ForceSaveSceneEvent>()
            .add_systems(
                Update,
                (
                    handle_save_scene,
                    handle_force_save_scene,
                    handle_load_scene,
                    detect_scene_changes,
                    update_window_title,
                ),
            )
            .add_systems(EguiPrimaryContextPass, (draw_error_dialog, draw_overwrite_dialog));
    }
}

/// Handle save scene events - check for existing file first
fn handle_save_scene(
    mut events: MessageReader<SaveSceneEvent>,
    mut commands: Commands,
    camera_marks: Res<CameraMarks>,
    scene_file: Res<SceneFile>,
    mut overwrite_dialog: ResMut<OverwriteConfirmDialog>,
) {
    for event in events.read() {
        let path = Path::new(&event.path);

        // Check if file exists and it's not the currently open file
        let is_current_file = scene_file.path.as_ref() == Some(&event.path);

        if path.exists() && !is_current_file {
            // Show confirmation dialog
            overwrite_dialog.open = true;
            overwrite_dialog.path = event.path.clone();
        } else {
            // No existing file or saving to current file - proceed directly
            commands.queue(SaveSceneCommand {
                path: event.path.clone(),
                camera_marks: camera_marks.marks.clone(),
            });
        }
    }
}

/// Handle force save events (after confirmation)
fn handle_force_save_scene(
    mut events: MessageReader<ForceSaveSceneEvent>,
    mut commands: Commands,
    camera_marks: Res<CameraMarks>,
) {
    for event in events.read() {
        commands.queue(SaveSceneCommand {
            path: event.path.clone(),
            camera_marks: camera_marks.marks.clone(),
        });
    }
}

/// Command to save a scene with exclusive world access
struct SaveSceneCommand {
    path: String,
    camera_marks: HashMap<String, CameraMark>,
}

impl Command for SaveSceneCommand {
    fn apply(self, world: &mut World) {
        info!("SaveSceneCommand running for path: {}", self.path);

        // Collect scene entity IDs
        let scene_entity_ids: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
            query.iter(world).collect()
        };

        info!("Found {} scene entities to save", scene_entity_ids.len());

        // Build the scene, only allowing components we know can serialize
        let scene = DynamicSceneBuilder::from_world(world)
            .deny_all()
            .allow_component::<SceneEntity>()
            .allow_component::<Name>()
            .allow_component::<Transform>()
            .allow_component::<PrimitiveMarker>()
            .allow_component::<GroupMarker>()
            .allow_component::<Locked>()
            .allow_component::<SceneLightMarker>()
            .allow_component::<DirectionalLightMarker>()
            .allow_component::<GltfSource>()
            .allow_component::<SceneSource>()
            .allow_component::<RecursiveColliderConstructor>()
            .allow_component::<RigidBody>()
            .allow_component::<ChildOf>()
            .allow_component::<Children>()
            .extract_entities(scene_entity_ids.into_iter())
            .build();

        // Serialize the scene
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();

        info!("Serializing scene...");
        match scene.serialize(&type_registry) {
            Ok(serialized) => {
                // Write scene file
                if let Err(e) = fs::write(&self.path, &serialized) {
                    if let Some(mut error_dialog) = world.get_resource_mut::<SceneErrorDialog>() {
                        error_dialog.open = true;
                        error_dialog.title = "Save Error".to_string();
                        error_dialog.message = format!("Failed to write scene file:\n\n{}", e);
                    }
                    return;
                }

                // Write sidecar file with editor metadata
                let metadata = EditorMetadata {
                    camera_marks: self.camera_marks.clone(),
                };
                let metadata_path = format!("{}.meta", self.path);
                if let Ok(metadata_str) =
                    ron::ser::to_string_pretty(&metadata, ron::ser::PrettyConfig::default())
                {
                    let _ = fs::write(&metadata_path, metadata_str);
                }

                // Update SceneFile resource
                if let Some(mut scene_file) = world.get_resource_mut::<SceneFile>() {
                    scene_file.path = Some(self.path.clone());
                    scene_file.clear_modified();
                }

                info!("Scene saved to: {}", self.path);
            }
            Err(e) => {
                error!("Failed to serialize scene: {:?}", e);
                if let Some(mut error_dialog) = world.get_resource_mut::<SceneErrorDialog>() {
                    error_dialog.open = true;
                    error_dialog.title = "Save Error".to_string();
                    error_dialog.message = format!("Failed to serialize scene:\n\n{:?}", e);
                }
            }
        }
    }
}

fn handle_load_scene(
    mut events: MessageReader<LoadSceneEvent>,
    mut commands: Commands,
    existing: Query<Entity, With<SceneEntity>>,
    mut camera_marks: ResMut<CameraMarks>,
    mut error_dialog: ResMut<SceneErrorDialog>,
) {
    for event in events.read() {
        info!("Loading scene from: {}", event.path);

        // Read scene file
        let content = match fs::read_to_string(&event.path) {
            Ok(c) => c,
            Err(e) => {
                error_dialog.open = true;
                error_dialog.title = "Load Error".to_string();
                error_dialog.message = format!("Failed to read scene file:\n\n{}", e);
                continue;
            }
        };

        // Clear existing scene entities
        for entity in existing.iter() {
            commands.entity(entity).despawn();
        }

        // Store data needed for the command
        let path_clone = event.path.clone();

        // Queue the scene loading as a command (needs exclusive world access)
        commands.queue(LoadSceneCommand {
            content,
            path: path_clone,
        });

        // Load sidecar metadata if it exists
        let metadata_path = format!("{}.meta", event.path);
        if let Ok(metadata_content) = fs::read_to_string(&metadata_path) {
            if let Ok(metadata) = ron::from_str::<EditorMetadata>(&metadata_content) {
                if !metadata.camera_marks.is_empty() {
                    camera_marks.marks = metadata.camera_marks;
                    info!("Loaded camera marks from metadata");
                }
            }
        }
    }
}

/// Command to load a scene with exclusive world access
struct LoadSceneCommand {
    content: String,
    path: String,
}

impl Command for LoadSceneCommand {
    fn apply(self, world: &mut World) {
        // Deserialize scene
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();
        let scene_deserializer = SceneDeserializer {
            type_registry: &type_registry,
        };

        let mut ron_deserializer = match ron::de::Deserializer::from_str(&self.content) {
            Ok(d) => d,
            Err(e) => {
                error!("Failed to parse scene file: {}", e);
                if let Some(mut error_dialog) = world.get_resource_mut::<SceneErrorDialog>() {
                    error_dialog.open = true;
                    error_dialog.title = "Parse Error".to_string();
                    error_dialog.message = format!("Failed to parse scene file:\n\n{}", e);
                }
                return;
            }
        };

        let scene: DynamicScene = match scene_deserializer.deserialize(&mut ron_deserializer) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to deserialize scene: {:?}", e);
                if let Some(mut error_dialog) = world.get_resource_mut::<SceneErrorDialog>() {
                    error_dialog.open = true;
                    error_dialog.title = "Deserialize Error".to_string();
                    error_dialog.message = format!("Failed to deserialize scene:\n\n{:?}", e);
                }
                return;
            }
        };

        // Need to drop the type registry borrow before writing to world
        drop(type_registry);

        // Write scene to world
        let mut entity_map = bevy::ecs::entity::EntityHashMap::default();
        if let Err(e) = scene.write_to_world(world, &mut entity_map) {
            error!("Failed to instantiate scene: {:?}", e);
            if let Some(mut error_dialog) = world.get_resource_mut::<SceneErrorDialog>() {
                error_dialog.open = true;
                error_dialog.title = "Load Error".to_string();
                error_dialog.message = format!("Failed to instantiate scene:\n\n{:?}", e);
            }
            return;
        }

        // Regenerate meshes and materials from PrimitiveMarker
        regenerate_meshes(world);

        // Note: Physics state is preserved - not changed on load

        // Update SceneFile resource
        if let Some(mut scene_file) = world.get_resource_mut::<SceneFile>() {
            scene_file.path = Some(self.path.clone());
            scene_file.clear_modified();
        }

        let scene_name = Path::new(&self.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled");
        info!("Scene loaded: {}", scene_name);
    }
}

/// Regenerate meshes and materials for entities loaded from scene
/// This is called after loading a scene to create the actual Mesh3d, MeshMaterial3d,
/// PointLight, DirectionalLight, and Collider components from the marker components.
pub fn regenerate_meshes(world: &mut World) {
    // Get all entities with PrimitiveMarker but no Mesh3d
    let mut entities_to_update: Vec<(Entity, PrimitiveShape)> = Vec::new();

    {
        let mut query = world.query_filtered::<(Entity, &PrimitiveMarker), Without<Mesh3d>>();
        for (entity, marker) in query.iter(world) {
            entities_to_update.push((entity, marker.shape));
        }
    }

    // Add meshes and materials using PrimitiveShape helper methods
    for (entity, shape) in entities_to_update {
        let mesh_handle = world.resource_mut::<Assets<Mesh>>().add(shape.create_mesh());
        let material_handle = world
            .resource_mut::<Assets<StandardMaterial>>()
            .add(shape.create_material());
        let collider = shape.create_collider();

        // Insert components
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((
                Mesh3d(mesh_handle),
                MeshMaterial3d(material_handle),
                collider,
            ));
        }
    }

    // Also sync PointLight from SceneLightMarker for lights
    let mut lights_to_update: Vec<(Entity, SceneLightMarker)> = Vec::new();
    {
        let mut query = world.query_filtered::<(Entity, &SceneLightMarker), Without<PointLight>>();
        for (entity, marker) in query.iter(world) {
            lights_to_update.push((entity, marker.clone()));
        }
    }

    for (entity, marker) in lights_to_update {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((
                PointLight {
                    color: marker.color,
                    intensity: marker.intensity,
                    range: marker.range,
                    shadows_enabled: marker.shadows_enabled,
                    ..default()
                },
                Visibility::default(),
                // Collider for selection via raycasting
                Collider::sphere(LIGHT_COLLIDER_RADIUS),
            ));
        }
    }

    // Also sync DirectionalLight from DirectionalLightMarker
    let mut dir_lights_to_update: Vec<(Entity, DirectionalLightMarker)> = Vec::new();
    {
        let mut query = world.query_filtered::<(Entity, &DirectionalLightMarker), Without<DirectionalLight>>();
        for (entity, marker) in query.iter(world) {
            dir_lights_to_update.push((entity, marker.clone()));
        }
    }

    for (entity, marker) in dir_lights_to_update {
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((
                DirectionalLight {
                    color: marker.color,
                    illuminance: marker.illuminance,
                    shadows_enabled: marker.shadows_enabled,
                    ..default()
                },
                Visibility::default(),
                // Collider for selection via raycasting
                Collider::sphere(LIGHT_COLLIDER_RADIUS),
            ));
        }
    }
}

/// Draw the error dialog if it's open
fn draw_error_dialog(
    mut contexts: EguiContexts,
    mut error_dialog: ResMut<SceneErrorDialog>,
) -> Result {
    if !error_dialog.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    if draw_themed_error_dialog(ctx, &error_dialog.title, &error_dialog.message) {
        error_dialog.open = false;
    }

    Ok(())
}

/// Detect changes to scene entities and mark scene as modified
fn detect_scene_changes(
    mut scene_file: ResMut<SceneFile>,
    changed_transforms: Query<(), (With<SceneEntity>, Changed<Transform>)>,
    changed_names: Query<(), (With<SceneEntity>, Changed<Name>)>,
    changed_primitives: Query<(), (With<SceneEntity>, Changed<PrimitiveMarker>)>,
    changed_lights: Query<(), (With<SceneEntity>, Changed<SceneLightMarker>)>,
    changed_bodies: Query<(), (With<SceneEntity>, Changed<RigidBody>)>,
    added_entities: Query<(), Added<SceneEntity>>,
    mut removed_entities: RemovedComponents<SceneEntity>,
) {
    // Skip if already modified
    if scene_file.modified {
        return;
    }

    // Check for any changes
    let has_changes = !changed_transforms.is_empty()
        || !changed_names.is_empty()
        || !changed_primitives.is_empty()
        || !changed_lights.is_empty()
        || !changed_bodies.is_empty()
        || !added_entities.is_empty()
        || removed_entities.read().next().is_some();

    if has_changes {
        scene_file.mark_modified();
    }
}

/// Update the window title to reflect current file and modification state
fn update_window_title(
    scene_file: Res<SceneFile>,
    mut window_query: Query<&mut Window, With<PrimaryWindow>>,
) {
    // Only update when SceneFile changes
    if !scene_file.is_changed() {
        return;
    }

    let Ok(mut window) = window_query.single_mut() else {
        return;
    };

    let file_name = scene_file.display_name();
    let modified_indicator = if scene_file.modified { " *" } else { "" };

    window.title = format!("Bevy Avian3D Editor - {}{}", file_name, modified_indicator);
}

/// Draw the overwrite confirmation dialog
fn draw_overwrite_dialog(
    mut contexts: EguiContexts,
    mut overwrite_dialog: ResMut<OverwriteConfirmDialog>,
    mut force_save_events: MessageWriter<ForceSaveSceneEvent>,
) -> Result {
    if !overwrite_dialog.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let mut should_close = false;
    let mut should_save = false;

    egui::Window::new("Confirm Overwrite")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let filename = Path::new(&overwrite_dialog.path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&overwrite_dialog.path);

            ui.label(format!("File '{}' already exists.", filename));
            ui.label("Do you want to overwrite it?");
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if ui.button("Overwrite").clicked() {
                    should_save = true;
                    should_close = true;
                }
                if ui.button("Cancel").clicked() {
                    should_close = true;
                }
            });
        });

    if should_save {
        force_save_events.write(ForceSaveSceneEvent {
            path: overwrite_dialog.path.clone(),
        });
    }

    if should_close {
        overwrite_dialog.open = false;
        overwrite_dialog.path.clear();
    }

    Ok(())
}
