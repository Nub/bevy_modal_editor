use bevy::prelude::*;
use bevy_egui::EguiContexts;

use super::TakeSnapshotCommand;
use crate::editor::{EditorMode, EditorState};
use crate::scene::blockout::{ArchMarker, LShapeMarker, RampMarker, StairsMarker};
use crate::scene::{
    DirectionalLightMarker, FogVolumeMarker, GroupMarker, PrimitiveMarker, SceneEntity,
    SceneLightMarker, SpawnEntityEvent, SpawnEntityKind, SplineMarker,
};
use crate::selection::Selected;
use crate::utils::should_process_input;

/// Event to delete selected entities
#[derive(Message)]
pub struct DeleteSelectedEvent;

/// Event to duplicate selected entities
#[derive(Message)]
pub struct DuplicateSelectedEvent;

/// Event to nudge selected entities by a grid step
#[derive(Message)]
pub struct NudgeSelectedEvent {
    pub direction: Vec3,
}

pub struct OperationsPlugin;

impl Plugin for OperationsPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<DeleteSelectedEvent>()
            .add_message::<DuplicateSelectedEvent>()
            .add_message::<NudgeSelectedEvent>()
            .add_systems(
                Update,
                (
                    handle_delete_input,
                    handle_nudge_input,
                    handle_delete_selected,
                    handle_duplicate_selected,
                    handle_nudge_selected,
                ),
            );
    }
}

fn handle_delete_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    editor_mode: Res<State<EditorMode>>,
    mut delete_events: MessageWriter<DeleteSelectedEvent>,
    mut duplicate_events: MessageWriter<DuplicateSelectedEvent>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Delete key always deletes, X only deletes outside ObjectInspector mode
    // (in ObjectInspector mode, X opens the remove component palette instead)
    let x_pressed =
        keyboard.just_pressed(KeyCode::KeyX) && *editor_mode.get() != EditorMode::ObjectInspector;
    if keyboard.just_pressed(KeyCode::Delete) || x_pressed {
        delete_events.write(DeleteSelectedEvent);
    }

    // Ctrl+D to duplicate
    let ctrl = keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);
    if ctrl && keyboard.just_pressed(KeyCode::KeyD) {
        duplicate_events.write(DuplicateSelectedEvent);
    }
}

/// Handle arrow key input for nudging selected entities
fn handle_nudge_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    editor_mode: Res<State<EditorMode>>,
    mut nudge_events: MessageWriter<NudgeSelectedEvent>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Only nudge in View or Edit mode (not in Blockout mode which uses WASDQE differently)
    if *editor_mode.get() != EditorMode::View && *editor_mode.get() != EditorMode::Edit {
        return;
    }

    // Use grid snap amount or 0.25 as default
    let nudge_amount = if editor_state.grid_snap > 0.0 {
        editor_state.grid_snap
    } else {
        0.25
    };

    // Arrow keys for XZ plane movement
    if keyboard.just_pressed(KeyCode::ArrowUp) {
        nudge_events.write(NudgeSelectedEvent {
            direction: Vec3::new(0.0, 0.0, -nudge_amount),
        });
    }
    if keyboard.just_pressed(KeyCode::ArrowDown) {
        nudge_events.write(NudgeSelectedEvent {
            direction: Vec3::new(0.0, 0.0, nudge_amount),
        });
    }
    if keyboard.just_pressed(KeyCode::ArrowLeft) {
        nudge_events.write(NudgeSelectedEvent {
            direction: Vec3::new(-nudge_amount, 0.0, 0.0),
        });
    }
    if keyboard.just_pressed(KeyCode::ArrowRight) {
        nudge_events.write(NudgeSelectedEvent {
            direction: Vec3::new(nudge_amount, 0.0, 0.0),
        });
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

/// Query for all duplicatable entity types
#[allow(clippy::type_complexity)]
fn handle_duplicate_selected(
    mut events: MessageReader<DuplicateSelectedEvent>,
    selected_query: Query<
        (
            Entity,
            &Transform,
            Option<&PrimitiveMarker>,
            Option<&GroupMarker>,
            Option<&SceneLightMarker>,
            Option<&DirectionalLightMarker>,
            Option<&SplineMarker>,
            Option<&FogVolumeMarker>,
            Option<&StairsMarker>,
            Option<&RampMarker>,
            Option<&ArchMarker>,
            Option<&LShapeMarker>,
        ),
        With<Selected>,
    >,
    mut spawn_events: MessageWriter<SpawnEntityEvent>,
    mut commands: Commands,
) {
    use bevy_spline_3d::prelude::SplineType;

    for _ in events.read() {
        let selected: Vec<_> = selected_query.iter().collect();
        let count = selected.len();
        if count == 0 {
            continue;
        }

        // Queue snapshot command before spawning duplicates
        commands.queue(TakeSnapshotCommand {
            description: format!("Duplicate {} entities", count),
        });

        for (
            _entity,
            transform,
            primitive,
            group,
            point_light,
            dir_light,
            spline,
            fog,
            stairs,
            ramp,
            arch,
            lshape,
        ) in selected
        {
            // Duplicate in-place (no offset) - use arrow keys to nudge
            let position = transform.translation;
            let rotation = transform.rotation;

            // Determine what kind of entity to spawn
            let kind = if let Some(prim) = primitive {
                Some(SpawnEntityKind::Primitive(prim.shape))
            } else if group.is_some() {
                Some(SpawnEntityKind::Group)
            } else if point_light.is_some() {
                Some(SpawnEntityKind::PointLight)
            } else if dir_light.is_some() {
                Some(SpawnEntityKind::DirectionalLight)
            } else if spline.is_some() {
                // Default to CatmullRom for duplicated splines
                Some(SpawnEntityKind::Spline(SplineType::CatmullRom))
            } else if fog.is_some() {
                Some(SpawnEntityKind::FogVolume)
            } else if stairs.is_some() {
                Some(SpawnEntityKind::Stairs)
            } else if ramp.is_some() {
                Some(SpawnEntityKind::Ramp)
            } else if arch.is_some() {
                Some(SpawnEntityKind::Arch)
            } else if lshape.is_some() {
                Some(SpawnEntityKind::LShape)
            } else {
                None
            };

            if let Some(kind) = kind {
                spawn_events.write(SpawnEntityEvent {
                    kind,
                    position,
                    rotation,
                });
            }
        }
    }
}

/// Handle nudging selected entities
fn handle_nudge_selected(
    mut events: MessageReader<NudgeSelectedEvent>,
    mut selected_query: Query<&mut Transform, (With<Selected>, With<SceneEntity>)>,
    mut commands: Commands,
) {
    for event in events.read() {
        let count = selected_query.iter().count();
        if count == 0 {
            continue;
        }

        // Take snapshot for undo
        commands.queue(TakeSnapshotCommand {
            description: format!("Nudge {} entities", count),
        });

        for mut transform in selected_query.iter_mut() {
            transform.translation += event.direction;
        }
    }
}
