use avian3d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use super::{PrimitiveMarker, PrimitiveShape, SceneEntity};
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

pub struct SerializationPlugin;

impl Plugin for SerializationPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SaveSceneEvent>()
            .add_message::<LoadSceneEvent>()
            .add_systems(Update, (handle_save_scene, handle_load_scene));
    }
}

fn handle_save_scene(
    mut events: MessageReader<SaveSceneEvent>,
    entities: Query<
        (&Name, &Transform, Option<&PrimitiveMarker>, Option<&RigidBody>),
        With<SceneEntity>,
    >,
    camera_marks: Res<CameraMarks>,
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

        for (name, transform, primitive, rigid_body) in entities.iter() {
            scene.entities.push(SerializedEntity {
                name: name.as_str().to_string(),
                transform: SerializedTransform::from(transform),
                primitive: primitive.map(|p| p.shape),
                rigid_body: rigid_body.map(SerializedRigidBody::from),
            });
        }

        match ron::ser::to_string_pretty(&scene, ron::ser::PrettyConfig::default()) {
            Ok(ron_string) => {
                if let Err(e) = fs::write(&event.path, ron_string) {
                    error!("Failed to write scene file: {}", e);
                } else {
                    info!("Scene saved to: {}", event.path);
                }
            }
            Err(e) => {
                error!("Failed to serialize scene: {}", e);
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
) {
    for event in events.read() {
        let content = match fs::read_to_string(&event.path) {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to read scene file: {}", e);
                continue;
            }
        };

        let scene: EditorScene = match ron::from_str(&content) {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to parse scene file: {}", e);
                continue;
            }
        };

        // Clear existing scene entities
        for entity in existing.iter() {
            commands.entity(entity).despawn();
        }

        // Spawn loaded entities
        for entity_data in scene.entities {
            let transform: Transform = (&entity_data.transform).into();

            let mut entity_commands = commands.spawn((
                SceneEntity,
                Name::new(entity_data.name),
                transform,
            ));

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
            if let Some(rb) = entity_data.rigid_body {
                entity_commands.insert(RigidBody::from(&rb));
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
