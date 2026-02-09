use bevy::prelude::*;
use bevy_egui::EguiContexts;

use bevy_editor_game::GameState;

use super::state::{AxisConstraint, ControlPointSnapState, CycleShadingModeEvent, EditorMode, EditorState, PinnedWindows, ToggleEditorEvent, TogglePreviewModeEvent, TransformOperation};
use crate::commands::TakeSnapshotCommand;
use crate::scene::GroupSelectedEvent;
use crate::selection::Selected;
use crate::ui::{open_add_component_palette, CommandPaletteState, ComponentEditorState};
use crate::utils::should_process_input;

pub struct EditorInputPlugin;

impl Plugin for EditorInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (
            handle_editor_toggle,
            handle_mode_input,
            handle_group_shortcut,
            handle_preview_mode_shortcut,
            handle_measurement_toggle,
            handle_shading_mode_shortcut,
            handle_pin_window,
        ));
    }
}

/// Handle F10 to toggle the editor on/off
fn handle_editor_toggle(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut toggle_events: MessageWriter<ToggleEditorEvent>,
) {
    if keyboard.just_pressed(KeyCode::F10) {
        toggle_events.write(ToggleEditorEvent);
    }
}

/// Handle modal input switching and transform operation selection
fn handle_mode_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    current_mode: Res<State<EditorMode>>,
    mut next_mode: ResMut<NextState<EditorMode>>,
    mut transform_op: ResMut<TransformOperation>,
    mut axis_constraint: ResMut<AxisConstraint>,
    mut palette_state: ResMut<CommandPaletteState>,
    component_editor_state: Res<ComponentEditorState>,
    editor_state: Res<EditorState>,
    game_state: Res<State<GameState>>,
    snap_state: Res<ControlPointSnapState>,
    control_point_selection: Res<crate::editor::SelectedControlPointIndex>,
    selected_splines: Query<(), (With<Selected>, With<crate::scene::SplineMarker>)>,
    mut contexts: EguiContexts,
    mut commands: Commands,
    selected: Query<Entity, With<Selected>>,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Block mode switching when not in Editing state (e.g. during Paused)
    if *game_state.get() != GameState::Editing {
        return;
    }

    // In Edit mode with right mouse held, skip transform operations to allow camera movement
    let right_mouse_flying = *current_mode.get() == EditorMode::Edit && mouse_button.pressed(MouseButton::Right);

    let shift_held = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    let in_view_mode = *current_mode.get() == EditorMode::View;
    // Modes can only be entered from View mode, unless Shift is held
    let can_change_mode = in_view_mode || shift_held;

    // I key behavior depends on mode
    if keyboard.just_pressed(KeyCode::KeyI) {
        // In ObjectInspector mode, I opens the Add Component palette
        if *current_mode.get() == EditorMode::ObjectInspector {
            if let Some(entity) = selected.iter().next() {
                open_add_component_palette(&mut palette_state, entity);
            }
            return;
        }

        // If already in Insert mode, reopen the palette if closed
        if *current_mode.get() == EditorMode::Insert {
            if !palette_state.open {
                palette_state.open_insert();
            }
            return;
        }

        // Otherwise, enter Insert mode (only from View mode or with Shift)
        if can_change_mode {
            next_mode.set(EditorMode::Insert);
            *transform_op = TransformOperation::None;
            *axis_constraint = AxisConstraint::None;
            // Open command palette automatically in Insert mode
            palette_state.open_insert();
        }
        return;
    }

    // O enters Object Inspector mode (only from View mode or with Shift)
    if keyboard.just_pressed(KeyCode::KeyO) {
        if *current_mode.get() == EditorMode::ObjectInspector {
            // If already in ObjectInspector mode, return to View mode
            next_mode.set(EditorMode::View);
        } else if can_change_mode {
            next_mode.set(EditorMode::ObjectInspector);
            *transform_op = TransformOperation::None;
            *axis_constraint = AxisConstraint::None;
        }
        return;
    }

    // H enters Hierarchy mode (only from View mode or with Shift)
    if keyboard.just_pressed(KeyCode::KeyH) {
        if *current_mode.get() == EditorMode::Hierarchy {
            // If already in Hierarchy mode, return to View mode
            next_mode.set(EditorMode::View);
        } else if can_change_mode {
            next_mode.set(EditorMode::Hierarchy);
            *transform_op = TransformOperation::None;
            *axis_constraint = AxisConstraint::None;
        }
        return;
    }

    // B enters Blockout mode (only from View mode or with Shift)
    if keyboard.just_pressed(KeyCode::KeyB) {
        if *current_mode.get() == EditorMode::Blockout {
            // If already in Blockout mode, return to View mode
            next_mode.set(EditorMode::View);
        } else if can_change_mode {
            next_mode.set(EditorMode::Blockout);
            *transform_op = TransformOperation::None;
            *axis_constraint = AxisConstraint::None;
        }
        return;
    }

    // M enters Material mode (only from View mode or with Shift)
    if keyboard.just_pressed(KeyCode::KeyM) && !shift_held {
        if *current_mode.get() == EditorMode::Material {
            // If already in Material mode, return to View mode
            next_mode.set(EditorMode::View);
        } else if can_change_mode {
            next_mode.set(EditorMode::Material);
            *transform_op = TransformOperation::None;
            *axis_constraint = AxisConstraint::None;
        }
        return;
    }

    // V enters Camera mode (only from View mode or with Shift)
    if keyboard.just_pressed(KeyCode::KeyV) {
        if *current_mode.get() == EditorMode::Camera {
            next_mode.set(EditorMode::View);
        } else if can_change_mode {
            next_mode.set(EditorMode::Camera);
            *transform_op = TransformOperation::None;
            *axis_constraint = AxisConstraint::None;
        }
        return;
    }

    // N enters Particle mode (only from View mode or with Shift)
    if keyboard.just_pressed(KeyCode::KeyN) {
        if *current_mode.get() == EditorMode::Particle {
            next_mode.set(EditorMode::View);
        } else if can_change_mode {
            next_mode.set(EditorMode::Particle);
            *transform_op = TransformOperation::None;
            *axis_constraint = AxisConstraint::None;
        }
        return;
    }

    // ; enters AI mode (only from View mode or with Shift)
    if keyboard.just_pressed(KeyCode::Semicolon) {
        if *current_mode.get() == EditorMode::AI {
            next_mode.set(EditorMode::View);
        } else if can_change_mode {
            next_mode.set(EditorMode::AI);
            *transform_op = TransformOperation::None;
            *axis_constraint = AxisConstraint::None;
        }
        return;
    }

    // Escape returns to View mode from any mode, unless a popup is open
    // (let the popup handle Escape first)
    if keyboard.just_pressed(KeyCode::Escape) {
        // Don't change mode if component editor popup is open - let it close first
        if component_editor_state.editing_component.is_some() {
            return;
        }

        // Don't change mode if control point snap is active - let the snap confirm handle it
        if snap_state.active {
            return;
        }

        if *current_mode.get() != EditorMode::View {
            next_mode.set(EditorMode::View);
            *transform_op = TransformOperation::None;
            *axis_constraint = AxisConstraint::None;
        }
        return;
    }

    // E enters Edit mode from View, or sets Scale in Edit mode
    if keyboard.just_pressed(KeyCode::KeyE) {
        match current_mode.get() {
            EditorMode::View => {
                next_mode.set(EditorMode::Edit);
            }
            EditorMode::Edit => {
                *transform_op = TransformOperation::Scale;
                *axis_constraint = AxisConstraint::None;
            }
            _ if shift_held => {
                // With Shift, can enter Edit mode from any mode
                next_mode.set(EditorMode::Edit);
            }
            EditorMode::Insert | EditorMode::ObjectInspector | EditorMode::Hierarchy | EditorMode::Blockout | EditorMode::Material | EditorMode::Camera | EditorMode::Particle | EditorMode::AI => {}
        }
        return;
    }

    // Transform operations only in Edit mode (but not when right-click flying)
    // Q = Translate, W = Rotate, R = Place, T = Snap to Object
    if *current_mode.get() == EditorMode::Edit && !right_mouse_flying {
        if keyboard.just_pressed(KeyCode::KeyQ) {
            *transform_op = TransformOperation::Translate;
            *axis_constraint = AxisConstraint::None;
        } else if keyboard.just_pressed(KeyCode::KeyW) {
            *transform_op = TransformOperation::Rotate;
            *axis_constraint = AxisConstraint::None;
        } else if keyboard.just_pressed(KeyCode::KeyR) {
            // Take snapshot before entering place mode
            if !selected.is_empty() {
                commands.queue(TakeSnapshotCommand {
                    description: "Place entities".to_string(),
                });
            }
            *transform_op = TransformOperation::Place;
            *axis_constraint = AxisConstraint::None;
        } else if keyboard.just_pressed(KeyCode::KeyT) {
            // If a spline with a selected control point is active, let the
            // spline hotkey handler deal with T (control point snap) instead
            let spline_has_selected_point = !selected_splines.is_empty()
                && control_point_selection.0.is_some();
            if !spline_has_selected_point {
                // Take snapshot before entering snap to object mode
                if !selected.is_empty() {
                    commands.queue(TakeSnapshotCommand {
                        description: "Snap to object".to_string(),
                    });
                }
                *transform_op = TransformOperation::SnapToObject;
                *axis_constraint = AxisConstraint::None;
            }
        }
        // Axis selection (A, S, D) is handled in gizmos/transform.rs
    }
}

/// Handle G key to group selected entities
fn handle_group_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    mut group_events: MessageWriter<GroupSelectedEvent>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // G to group selected entities
    if keyboard.just_pressed(KeyCode::KeyG) {
        group_events.write(GroupSelectedEvent);
    }
}

/// Handle P key to toggle preview mode (only in View mode)
fn handle_preview_mode_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    mode: Res<State<EditorMode>>,
    mut preview_events: MessageWriter<TogglePreviewModeEvent>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // P to toggle preview mode (only in View mode)
    // In Material mode, P is used for pasting materials
    if keyboard.just_pressed(KeyCode::KeyP) && *mode.get() == EditorMode::View {
        preview_events.write(TogglePreviewModeEvent);
    }
}

/// Handle Shift+M to toggle distance measurements
fn handle_measurement_toggle(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut editor_state: ResMut<EditorState>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Shift+M to toggle measurements
    let shift_held = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if shift_held && keyboard.just_pressed(KeyCode::KeyM) {
        editor_state.measurements_visible = !editor_state.measurements_visible;
        info!(
            "Measurements: {}",
            if editor_state.measurements_visible { "ON" } else { "OFF" }
        );
    }
}

/// Handle Z key to cycle viewport shading modes (View mode only)
fn handle_shading_mode_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    mode: Res<State<EditorMode>>,
    mut cycle_events: MessageWriter<CycleShadingModeEvent>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Z to cycle shading modes (only in View mode)
    if keyboard.just_pressed(KeyCode::KeyZ) && *mode.get() == EditorMode::View {
        cycle_events.write(CycleShadingModeEvent);
    }
}

/// Handle "." key to toggle pinning the current mode's window
fn handle_pin_window(
    keyboard: Res<ButtonInput<KeyCode>>,
    current_mode: Res<State<EditorMode>>,
    editor_state: Res<EditorState>,
    game_state: Res<State<GameState>>,
    mut pinned_window: ResMut<PinnedWindows>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    if *game_state.get() != GameState::Editing {
        return;
    }

    if keyboard.just_pressed(KeyCode::Period) {
        let mode = *current_mode.get();
        if mode.has_panel() {
            if !pinned_window.0.remove(&mode) {
                pinned_window.0.insert(mode);
            }
        }
    }
}
