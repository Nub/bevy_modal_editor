use bevy::prelude::*;
use bevy_egui::EguiContexts;

use super::TakeSnapshotCommand;
use crate::editor::EditorState;
use crate::scene::{SpawnEntityEvent, SpawnEntityKind};
use crate::selection::Selected;
use crate::utils::should_process_input;

/// Event to delete selected entities
#[derive(Message)]
pub struct DeleteSelectedEvent;

/// Event to duplicate selected entities
#[derive(Message)]
pub struct DuplicateSelectedEvent;

pub struct OperationsPlugin;

impl Plugin for OperationsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DeleteSelectedEvent>()
            .add_message::<DuplicateSelectedEvent>()
            .add_systems(
                Update,
                (handle_delete_input, handle_delete_selected, handle_duplicate_selected),
            );
    }
}

fn handle_delete_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    mut delete_events: MessageWriter<DeleteSelectedEvent>,
    mut duplicate_events: MessageWriter<DuplicateSelectedEvent>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Delete or X to delete selected
    if keyboard.just_pressed(KeyCode::Delete) || keyboard.just_pressed(KeyCode::KeyX) {
        delete_events.write(DeleteSelectedEvent);
    }

    // Ctrl+D to duplicate
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if ctrl && keyboard.just_pressed(KeyCode::KeyD) {
        duplicate_events.write(DuplicateSelectedEvent);
    }
}

fn handle_delete_selected(
    mut events: MessageReader<DeleteSelectedEvent>,
    selected: Query<Entity, With<Selected>>,
    mut commands: Commands,
) {
    for _ in events.read() {
        let count = selected.iter().count();
        if count > 0 {
            // Queue snapshot command first, then despawn
            commands.queue(TakeSnapshotCommand {
                description: format!("Delete {} entities", count),
            });

            for entity in selected.iter() {
                commands.entity(entity).despawn();
            }
            info!("Deleted {} entities", count);
        }
    }
}

fn handle_duplicate_selected(
    mut events: MessageReader<DuplicateSelectedEvent>,
    selected: Query<(&Transform, &crate::scene::PrimitiveMarker), With<Selected>>,
    mut spawn_events: MessageWriter<SpawnEntityEvent>,
    mut commands: Commands,
) {
    for _ in events.read() {
        let count = selected.iter().count();
        if count > 0 {
            // Queue snapshot command before spawning duplicates
            commands.queue(TakeSnapshotCommand {
                description: format!("Duplicate {} entities", count),
            });

            for (transform, primitive) in selected.iter() {
                // Offset the duplicated entity slightly
                let offset = Vec3::new(1.0, 0.0, 1.0);
                spawn_events.write(SpawnEntityEvent {
                    kind: SpawnEntityKind::Primitive(primitive.shape),
                    position: transform.translation + offset,
                    rotation: transform.rotation,
                });
            }
        }
    }
}
