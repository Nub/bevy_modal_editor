use avian3d::prelude::{Collider, Physics, RigidBody};
use avian3d::schedule::PhysicsTime;
use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;
use bevy::scene::serde::SceneDeserializer;
use bevy_egui::EguiContexts;
use serde::de::DeserializeSeed;
use std::collections::VecDeque;

use crate::scene::{
    DirectionalLightMarker, GroupMarker, Locked, PrimitiveMarker, PrimitiveShape, SceneEntity,
    SceneLightMarker, LIGHT_COLLIDER_RADIUS,
};
use crate::ui::Settings;

/// A snapshot of the scene state for undo/redo
#[derive(Clone)]
struct SceneSnapshot {
    /// Serialized scene data (RON format)
    data: String,
    /// Description of what action this snapshot is for
    description: String,
}

/// Resource to manage undo/redo history using scene snapshots
#[derive(Resource)]
pub struct SnapshotHistory {
    /// Stack of previous states (for undo)
    undo_stack: VecDeque<SceneSnapshot>,
    /// Stack of future states (for redo)
    redo_stack: VecDeque<SceneSnapshot>,
    /// Whether we're currently restoring (to avoid taking snapshots during restore)
    restoring: bool,
}

impl Default for SnapshotHistory {
    fn default() -> Self {
        Self {
            undo_stack: VecDeque::with_capacity(50),
            redo_stack: VecDeque::with_capacity(50),
            restoring: false,
        }
    }
}

impl SnapshotHistory {
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo_count(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_count(&self) -> usize {
        self.redo_stack.len()
    }
}

/// Event to trigger undo
#[derive(Message)]
pub struct UndoEvent;

/// Event to trigger redo
#[derive(Message)]
pub struct RedoEvent;

pub struct HistoryPlugin;

impl Plugin for HistoryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SnapshotHistory>()
            .add_message::<UndoEvent>()
            .add_message::<RedoEvent>()
            .add_systems(
                Update,
                (
                    handle_undo_redo_input,
                    handle_undo,
                    handle_redo,
                ),
            );
    }
}

fn handle_undo_redo_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut undo_events: MessageWriter<UndoEvent>,
    mut redo_events: MessageWriter<RedoEvent>,
    mut contexts: EguiContexts,
) {
    // Don't handle when UI wants keyboard input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }

    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);

    // U for undo
    if keyboard.just_pressed(KeyCode::KeyU) {
        info!("U pressed - sending undo event");
        undo_events.write(UndoEvent);
    }

    // Ctrl+R for redo
    if ctrl && keyboard.just_pressed(KeyCode::KeyR) {
        redo_events.write(RedoEvent);
    }
}

/// Command to take a snapshot with exclusive world access
pub struct TakeSnapshotCommand {
    pub description: String,
}

impl Command for TakeSnapshotCommand {
    fn apply(self, world: &mut World) {
        // Check if we're currently restoring - don't take snapshots during restore
        let restoring = world
            .get_resource::<SnapshotHistory>()
            .map(|h| h.restoring)
            .unwrap_or(false);

        if restoring {
            info!("Skipping snapshot (restoring): {}", self.description);
            return;
        }

        // Collect scene entity IDs
        let scene_entity_ids: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
            query.iter(world).collect()
        };

        info!("Taking snapshot '{}' with {} entities", self.description, scene_entity_ids.len());

        // Build the scene
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
            .allow_component::<RigidBody>()
            .allow_component::<ChildOf>()
            .allow_component::<Children>()
            .extract_entities(scene_entity_ids.into_iter())
            .build();

        // Serialize the scene
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();

        let Ok(serialized) = scene.serialize(&type_registry) else {
            warn!("Failed to serialize scene for snapshot");
            return;
        };

        drop(type_registry);

        // Get max history size from settings
        let max_history = world
            .get_resource::<Settings>()
            .map(|s| s.undo_history_size)
            .unwrap_or(50);

        // Add to history
        if let Some(mut history) = world.get_resource_mut::<SnapshotHistory>() {
            // Clear redo stack when new action is taken
            history.redo_stack.clear();

            // Add to undo stack
            while history.undo_stack.len() >= max_history {
                history.undo_stack.pop_front();
            }

            history.undo_stack.push_back(SceneSnapshot {
                data: serialized,
                description: self.description,
            });
        }
    }
}

/// Handle undo events
fn handle_undo(mut events: MessageReader<UndoEvent>, mut commands: Commands) {
    for _ in events.read() {
        info!("handle_undo: received UndoEvent, queueing UndoCommand");
        commands.queue(UndoCommand);
    }
}

/// Command to perform undo with exclusive world access
struct UndoCommand;

impl Command for UndoCommand {
    fn apply(self, world: &mut World) {
        info!("UndoCommand::apply running");

        // First, take a snapshot of current state for redo
        let current_snapshot = take_current_snapshot(world, "redo");

        // Get max history size from settings
        let max_history = world
            .get_resource::<Settings>()
            .map(|s| s.undo_history_size)
            .unwrap_or(50);

        // Pop from undo stack
        let snapshot = {
            let Some(mut history) = world.get_resource_mut::<SnapshotHistory>() else {
                info!("No SnapshotHistory resource found!");
                return;
            };

            info!(
                "Undo stack has {} items, redo stack has {} items",
                history.undo_stack.len(),
                history.redo_stack.len()
            );

            let Some(snapshot) = history.undo_stack.pop_back() else {
                info!("Nothing to undo");
                return;
            };

            // Push current state to redo stack
            if let Some(current) = current_snapshot {
                while history.redo_stack.len() >= max_history {
                    history.redo_stack.pop_front();
                }
                history.redo_stack.push_back(current);
            }

            history.restoring = true;
            snapshot
        };

        // Restore the snapshot
        restore_snapshot(world, &snapshot);

        // Clear restoring flag
        if let Some(mut history) = world.get_resource_mut::<SnapshotHistory>() {
            history.restoring = false;
        }

        info!("Undo: {}", snapshot.description);
    }
}

/// Handle redo events
fn handle_redo(mut events: MessageReader<RedoEvent>, mut commands: Commands) {
    for _ in events.read() {
        commands.queue(RedoCommand);
    }
}

/// Command to perform redo with exclusive world access
struct RedoCommand;

impl Command for RedoCommand {
    fn apply(self, world: &mut World) {
        // First, take a snapshot of current state for undo
        let current_snapshot = take_current_snapshot(world, "undo");

        // Get max history size from settings
        let max_history = world
            .get_resource::<Settings>()
            .map(|s| s.undo_history_size)
            .unwrap_or(50);

        // Pop from redo stack
        let snapshot = {
            let Some(mut history) = world.get_resource_mut::<SnapshotHistory>() else {
                return;
            };

            let Some(snapshot) = history.redo_stack.pop_back() else {
                info!("Nothing to redo");
                return;
            };

            // Push current state to undo stack
            if let Some(current) = current_snapshot {
                while history.undo_stack.len() >= max_history {
                    history.undo_stack.pop_front();
                }
                history.undo_stack.push_back(current);
            }

            history.restoring = true;
            snapshot
        };

        // Restore the snapshot
        restore_snapshot(world, &snapshot);

        // Clear restoring flag
        if let Some(mut history) = world.get_resource_mut::<SnapshotHistory>() {
            history.restoring = false;
        }

        info!("Redo: {}", snapshot.description);
    }
}

/// Take a snapshot of the current scene state
fn take_current_snapshot(world: &mut World, description: &str) -> Option<SceneSnapshot> {
    let scene_entity_ids: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
        query.iter(world).collect()
    };

    // Note: We allow empty scenes - an empty state is valid for undo/redo

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
        .allow_component::<RigidBody>()
        .allow_component::<ChildOf>()
        .allow_component::<Children>()
        .extract_entities(scene_entity_ids.into_iter())
        .build();

    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry = type_registry.read();

    scene.serialize(&type_registry).ok().map(|data| {
        SceneSnapshot {
            data,
            description: description.to_string(),
        }
    })
}

/// Restore the scene from a snapshot
fn restore_snapshot(world: &mut World, snapshot: &SceneSnapshot) {
    info!("Restoring snapshot: {}", snapshot.description);

    // Clear existing scene entities
    let entities_to_remove: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<SceneEntity>>();
        query.iter(world).collect()
    };

    info!("Removing {} existing scene entities", entities_to_remove.len());
    for entity in entities_to_remove {
        world.despawn(entity);
    }

    // Deserialize the snapshot
    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry = type_registry.read();

    let scene_deserializer = SceneDeserializer {
        type_registry: &type_registry,
    };

    let Ok(mut ron_deserializer) = ron::de::Deserializer::from_str(&snapshot.data) else {
        warn!("Failed to parse snapshot");
        return;
    };

    let Ok(scene) = scene_deserializer.deserialize(&mut ron_deserializer) else {
        warn!("Failed to deserialize snapshot");
        return;
    };

    drop(type_registry);

    // Write scene to world
    let mut entity_map = EntityHashMap::default();
    if let Err(e) = scene.write_to_world(world, &mut entity_map) {
        warn!("Failed to restore snapshot: {:?}", e);
        return;
    }

    info!("Wrote {} entities to world from snapshot", entity_map.len());

    // Regenerate meshes, materials, and colliders
    regenerate_scene_components(world);

    info!("Snapshot restoration complete");

    // Keep physics paused
    if let Some(mut physics_time) = world.get_resource_mut::<Time<Physics>>() {
        physics_time.set_relative_speed(0.0);
    }
}

/// Regenerate meshes and materials for entities loaded from snapshot
fn regenerate_scene_components(world: &mut World) {
    // Handle primitives
    let mut primitives_to_update: Vec<(Entity, PrimitiveShape)> = Vec::new();
    {
        let mut query = world.query_filtered::<(Entity, &PrimitiveMarker), Without<Mesh3d>>();
        for (entity, marker) in query.iter(world) {
            primitives_to_update.push((entity, marker.shape));
        }
    }

    for (entity, shape) in primitives_to_update {
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

            let mesh_handle = world.resource_mut::<Assets<Mesh>>().add(mesh);
            let material_handle = world
                .resource_mut::<Assets<StandardMaterial>>()
                .add(material);

            (mesh_handle, material_handle, collider)
        };

        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((Mesh3d(mesh_handle), MeshMaterial3d(material_handle), collider));
        }
    }

    // Handle point lights
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
                Collider::sphere(LIGHT_COLLIDER_RADIUS),
            ));
        }
    }

    // Handle directional lights
    let mut dir_lights_to_update: Vec<(Entity, DirectionalLightMarker)> = Vec::new();
    {
        let mut query =
            world.query_filtered::<(Entity, &DirectionalLightMarker), Without<DirectionalLight>>();
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
                Collider::sphere(LIGHT_COLLIDER_RADIUS),
            ));
        }
    }
}
