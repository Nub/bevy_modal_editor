use bevy::prelude::*;

use crate::scene::SpawnPrimitiveEvent;
use crate::selection::Selected;

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
    mut delete_events: MessageWriter<DeleteSelectedEvent>,
    mut duplicate_events: MessageWriter<DuplicateSelectedEvent>,
) {
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
        for entity in selected.iter() {
            commands.entity(entity).despawn();
        }
        if count > 0 {
            info!("Deleted {} entities", count);
        }
    }
}

fn handle_duplicate_selected(
    mut events: MessageReader<DuplicateSelectedEvent>,
    selected: Query<(&Transform, &crate::scene::PrimitiveMarker), With<Selected>>,
    mut spawn_events: MessageWriter<SpawnPrimitiveEvent>,
) {
    for _ in events.read() {
        for (transform, primitive) in selected.iter() {
            // Offset the duplicated entity slightly
            let offset = Vec3::new(1.0, 0.0, 1.0);
            spawn_events.write(SpawnPrimitiveEvent {
                shape: primitive.shape,
                position: transform.translation + offset,
            });
        }
    }
}
