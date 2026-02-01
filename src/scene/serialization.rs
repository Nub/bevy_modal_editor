use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::scene::serde::SceneDeserializer;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use serde::de::DeserializeSeed;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::{GroupMarker, PrimitiveMarker, PrimitiveShape, SceneEntity, SceneLightMarker};
use crate::editor::{CameraMark, CameraMarks};

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

/// Resource to store scene loading/saving errors for display
#[derive(Resource, Default)]
pub struct SceneErrorDialog {
    pub open: bool,
    pub title: String,
    pub message: String,
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
            .add_message::<SaveSceneEvent>()
            .add_message::<LoadSceneEvent>()
            .add_systems(Update, (handle_save_scene, handle_load_scene))
            .add_systems(EguiPrimaryContextPass, draw_error_dialog);
    }
}

/// Handle save scene events by queuing a command
fn handle_save_scene(
    mut events: MessageReader<SaveSceneEvent>,
    mut commands: Commands,
    camera_marks: Res<CameraMarks>,
) {
    for event in events.read() {
        // Queue the save operation as a command (needs exclusive world access for DynamicSceneBuilder)
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
            .allow_component::<SceneLightMarker>()
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

        let scene_name = Path::new(&self.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled");
        info!("Scene loaded: {}", scene_name);
    }
}

/// Regenerate meshes and materials for entities loaded from scene
fn regenerate_meshes(world: &mut World) {
    // Get all entities with PrimitiveMarker but no Mesh3d
    let mut entities_to_update: Vec<(Entity, PrimitiveShape)> = Vec::new();

    {
        let mut query = world.query_filtered::<(Entity, &PrimitiveMarker), Without<Mesh3d>>();
        for (entity, marker) in query.iter(world) {
            entities_to_update.push((entity, marker.shape));
        }
    }

    // Add meshes and materials - handle each shape one at a time to avoid borrow issues
    for (entity, shape) in entities_to_update {
        let (mesh_handle, material_handle, collider) = {
            let mesh = match shape {
                PrimitiveShape::Cube => Mesh::from(Cuboid::new(1.0, 1.0, 1.0)),
                PrimitiveShape::Sphere => Mesh::from(Sphere::new(0.5)),
                PrimitiveShape::Cylinder => Mesh::from(Cylinder::new(0.5, 1.0)),
                PrimitiveShape::Capsule => Mesh::from(Capsule3d::new(0.25, 0.5)),
                PrimitiveShape::Plane => Plane3d::default().mesh().size(2.0, 2.0).build(),
            };

            let material = match shape {
                PrimitiveShape::Cube => StandardMaterial {
                    base_color: Color::srgb(0.8, 0.7, 0.6),
                    ..default()
                },
                PrimitiveShape::Sphere => StandardMaterial {
                    base_color: Color::srgb(0.6, 0.7, 0.8),
                    ..default()
                },
                PrimitiveShape::Cylinder => StandardMaterial {
                    base_color: Color::srgb(0.7, 0.8, 0.6),
                    ..default()
                },
                PrimitiveShape::Capsule => StandardMaterial {
                    base_color: Color::srgb(0.8, 0.6, 0.7),
                    ..default()
                },
                PrimitiveShape::Plane => StandardMaterial {
                    base_color: Color::srgb(0.6, 0.6, 0.8),
                    ..default()
                },
            };

            let collider = match shape {
                PrimitiveShape::Cube => Collider::cuboid(1.0, 1.0, 1.0),
                PrimitiveShape::Sphere => Collider::sphere(0.5),
                PrimitiveShape::Cylinder => Collider::cylinder(0.5, 0.5),
                PrimitiveShape::Capsule => Collider::capsule(0.25, 0.5),
                PrimitiveShape::Plane => Collider::cuboid(2.0, 0.01, 2.0),
            };

            // Add to assets
            let mesh_handle = world.resource_mut::<Assets<Mesh>>().add(mesh);
            let material_handle = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(material);

            (mesh_handle, material_handle, collider)
        };

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

    egui::Window::new(&error_dialog.title)
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(&error_dialog.message);
            ui.add_space(10.0);
            if ui.button("OK").clicked() {
                error_dialog.open = false;
            }
        });
    Ok(())
}
