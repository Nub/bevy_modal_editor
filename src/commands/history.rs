use avian3d::prelude::Physics;
use avian3d::schedule::PhysicsTime;
use bevy::ecs::entity::EntityHashMap;
use bevy::prelude::*;
use bevy::scene::serde::SceneDeserializer;
use bevy_egui::EguiContexts;
use serde::de::DeserializeSeed;
use std::collections::VecDeque;

use crate::editor::EditorState;
use crate::scene::{build_editor_scene, regenerate_runtime_components, SceneEntity};
use crate::ui::Settings;
use crate::utils::should_process_input;

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
    editor_state: Res<EditorState>,
    mut undo_events: MessageWriter<UndoEvent>,
    mut redo_events: MessageWriter<RedoEvent>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
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

        // Build the scene using shared helper
        let scene = build_editor_scene(world, scene_entity_ids.into_iter());

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
    let scene = build_editor_scene(world, scene_entity_ids.into_iter());

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
    regenerate_runtime_components(world);

    info!("Snapshot restoration complete");

    // Keep physics paused
    if let Some(mut physics_time) = world.get_resource_mut::<Time<Physics>>() {
        physics_time.set_relative_speed(0.0);
    }
}
