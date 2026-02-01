use avian3d::prelude::*;
use bevy::ecs::relationship::Relationship;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::{GroupMarker, PrimitiveMarker, PrimitiveShape, SceneEntity, SceneLightMarker};
use crate::editor::{CameraMark, CameraMarks};

/// Serializable scene data
#[derive(Serialize, Deserialize)]
pub struct EditorScene {
    pub name: String,
    pub entities: Vec<SerializedEntity>,
    /// Camera marks (optional, for backwards compatibility)
    #[serde(default)]
    pub camera_marks: HashMap<String, CameraMark>,
}

/// Serializable entity data
#[derive(Serialize, Deserialize)]
pub struct SerializedEntity {
    pub name: String,
    pub transform: SerializedTransform,
    pub primitive: Option<PrimitiveShape>,
    pub rigid_body: Option<SerializedRigidBody>,
    /// Point light data (optional)
    #[serde(default)]
    pub light: Option<SerializedLight>,
    /// Whether this entity is a group (container)
    #[serde(default)]
    pub is_group: bool,
    /// Parent entity name (for hierarchy)
    #[serde(default)]
    pub parent: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct SerializedTransform {
    pub translation: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

impl From<&Transform> for SerializedTransform {
    fn from(t: &Transform) -> Self {
        Self {
            translation: t.translation.to_array(),
            rotation: t.rotation.to_array(),
            scale: t.scale.to_array(),
        }
    }
}

impl From<&SerializedTransform> for Transform {
    fn from(t: &SerializedTransform) -> Self {
        Transform {
            translation: Vec3::from_array(t.translation),
            rotation: Quat::from_array(t.rotation),
            scale: Vec3::from_array(t.scale),
        }
    }
}

#[derive(Serialize, Deserialize, Clone)]
pub enum SerializedRigidBody {
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
    fn from(rb: &SerializedRigidBody) -> Self {
        match rb {
            SerializedRigidBody::Static => RigidBody::Static,
            SerializedRigidBody::Dynamic => RigidBody::Dynamic,
            SerializedRigidBody::Kinematic => RigidBody::Kinematic,
        }
    }
}

/// Serializable point light data
#[derive(Serialize, Deserialize, Clone)]
pub struct SerializedLight {
    pub color: [f32; 4],
    pub intensity: f32,
    pub range: f32,
    pub shadows_enabled: bool,
}

impl From<&SceneLightMarker> for SerializedLight {
    fn from(light: &SceneLightMarker) -> Self {
        let rgba = light.color.to_linear();
        Self {
            color: [rgba.red, rgba.green, rgba.blue, rgba.alpha],
            intensity: light.intensity,
            range: light.range,
            shadows_enabled: light.shadows_enabled,
        }
    }
}

impl From<&SerializedLight> for SceneLightMarker {
    fn from(light: &SerializedLight) -> Self {
        Self {
            color: Color::linear_rgba(light.color[0], light.color[1], light.color[2], light.color[3]),
            intensity: light.intensity,
            range: light.range,
            shadows_enabled: light.shadows_enabled,
        }
    }
}

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

fn handle_save_scene(
    mut events: MessageReader<SaveSceneEvent>,
    entities: Query<
        (
            &Name,
            &Transform,
            Option<&PrimitiveMarker>,
            Option<&RigidBody>,
            Option<&GroupMarker>,
            Option<&SceneLightMarker>,
            Option<&ChildOf>,
        ),
        With<SceneEntity>,
    >,
    names: Query<&Name>,
    camera_marks: Res<CameraMarks>,
    mut error_dialog: ResMut<SceneErrorDialog>,
) {
    for event in events.read() {
        let mut scene = EditorScene {
            name: Path::new(&event.path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("Untitled")
                .to_string(),
            entities: Vec::new(),
            camera_marks: camera_marks.marks.clone(),
        };

        for (name, transform, primitive, rigid_body, group_marker, light_marker, parent) in entities.iter() {
            // Get parent name if it exists
            let parent_name = parent.and_then(|p| {
                names.get(p.get()).ok().map(|n| n.as_str().to_string())
            });

            scene.entities.push(SerializedEntity {
                name: name.as_str().to_string(),
                transform: SerializedTransform::from(transform),
                primitive: primitive.map(|p| p.shape),
                rigid_body: rigid_body.map(SerializedRigidBody::from),
                light: light_marker.map(SerializedLight::from),
                is_group: group_marker.is_some(),
                parent: parent_name,
            });
        }

        match ron::ser::to_string_pretty(&scene, ron::ser::PrettyConfig::default()) {
            Ok(ron_string) => {
                if let Err(e) = fs::write(&event.path, ron_string) {
                    error_dialog.open = true;
                    error_dialog.title = "Save Error".to_string();
                    error_dialog.message = format!("Failed to write scene file:\n\n{}", e);
                } else {
                    info!("Scene saved to: {}", event.path);
                }
            }
            Err(e) => {
                error_dialog.open = true;
                error_dialog.title = "Save Error".to_string();
                error_dialog.message = format!("Failed to serialize scene:\n\n{}", e);
            }
        }
    }
}

fn handle_load_scene(
    mut events: MessageReader<LoadSceneEvent>,
    mut commands: Commands,
    existing: Query<Entity, With<SceneEntity>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut camera_marks: ResMut<CameraMarks>,
    mut error_dialog: ResMut<SceneErrorDialog>,
) {
    for event in events.read() {
        let content = match fs::read_to_string(&event.path) {
            Ok(c) => c,
            Err(e) => {
                error_dialog.open = true;
                error_dialog.title = "Load Error".to_string();
                error_dialog.message = format!("Failed to read scene file:\n\n{}", e);
                continue;
            }
        };

        let scene: EditorScene = match ron::from_str::<EditorScene>(&content) {
            Ok(s) => s,
            Err(e) => {
                error_dialog.open = true;
                error_dialog.title = "Parse Error".to_string();
                error_dialog.message = format!("Failed to parse scene file:\n\n{}", e);
                continue;
            }
        };

        // Clear existing scene entities
        for entity in existing.iter() {
            commands.entity(entity).despawn();
        }

        // First pass: spawn all entities and build name -> entity map
        let mut name_to_entity: HashMap<String, Entity> = HashMap::new();
        let mut parent_info: Vec<(Entity, String)> = Vec::new();

        for entity_data in &scene.entities {
            let transform: Transform = (&entity_data.transform).into();

            let mut entity_commands = commands.spawn((
                SceneEntity,
                Name::new(entity_data.name.clone()),
                transform,
            ));

            // Add group marker if this is a group
            if entity_data.is_group {
                entity_commands.insert((GroupMarker, Visibility::default()));
            }

            // Add primitive mesh and collider
            if let Some(shape) = entity_data.primitive {
                entity_commands.insert(PrimitiveMarker { shape });

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

            // Add rigid body
            if let Some(rb) = &entity_data.rigid_body {
                entity_commands.insert(RigidBody::from(rb));
            }

            // Add point light
            if let Some(light_data) = &entity_data.light {
                let light_marker = SceneLightMarker::from(light_data);
                entity_commands.insert((
                    light_marker.clone(),
                    PointLight {
                        color: light_marker.color,
                        intensity: light_marker.intensity,
                        range: light_marker.range,
                        shadows_enabled: light_marker.shadows_enabled,
                        ..default()
                    },
                    Visibility::default(),
                ));
            }

            let entity = entity_commands.id();
            name_to_entity.insert(entity_data.name.clone(), entity);

            // Store parent info for second pass
            if let Some(parent_name) = &entity_data.parent {
                parent_info.push((entity, parent_name.clone()));
            }
        }

        // Second pass: set up parent-child relationships
        for (child, parent_name) in parent_info {
            if let Some(&parent) = name_to_entity.get(&parent_name) {
                commands.entity(child).set_parent_in_place(parent);
            } else {
                warn!("Parent '{}' not found for entity", parent_name);
            }
        }

        // Restore camera marks
        if !scene.camera_marks.is_empty() {
            camera_marks.marks = scene.camera_marks;
            info!("Loaded {} camera marks", camera_marks.marks.len());
        }

        info!("Scene loaded: {}", scene.name);
    }
}

/// Draw the error dialog if it's open
fn draw_error_dialog(mut contexts: EguiContexts, mut error_dialog: ResMut<SceneErrorDialog>) -> Result {
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
