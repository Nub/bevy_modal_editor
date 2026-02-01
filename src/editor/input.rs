use bevy::prelude::*;
use bevy_egui::EguiContexts;

use super::state::{AxisConstraint, EditorMode, TogglePreviewModeEvent, TransformOperation};
use crate::scene::GroupSelectedEvent;
use crate::ui::CommandPaletteState;

pub struct EditorInputPlugin;

impl Plugin for EditorInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (handle_mode_input, handle_group_shortcut, handle_preview_mode_shortcut));
    }
}

/// Handle modal input switching and transform operation selection
fn handle_mode_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    current_mode: Res<State<EditorMode>>,
    mut next_mode: ResMut<NextState<EditorMode>>,
    mut transform_op: ResMut<TransformOperation>,
    mut axis_constraint: ResMut<AxisConstraint>,
    mut palette_state: ResMut<CommandPaletteState>,
    mut contexts: EguiContexts,
) {
    // Don't handle shortcuts when UI wants keyboard input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }
    // V toggles between View and Edit modes
    if keyboard.just_pressed(KeyCode::KeyV) {
        match current_mode.get() {
            EditorMode::View => {
                next_mode.set(EditorMode::Edit);
            }
            EditorMode::Edit | EditorMode::Insert => {
                next_mode.set(EditorMode::View);
                *transform_op = TransformOperation::None;
                *axis_constraint = AxisConstraint::None;
            }
        }
        return;
    }

    // I enters Insert mode and opens command palette
    if keyboard.just_pressed(KeyCode::KeyI) {
        if *current_mode.get() != EditorMode::Insert {
            next_mode.set(EditorMode::Insert);
            *transform_op = TransformOperation::None;
            *axis_constraint = AxisConstraint::None;
            // Open command palette automatically
            palette_state.open = true;
            palette_state.query.clear();
            palette_state.selected_index = 0;
            palette_state.just_opened = true;
        }
        return;
    }

    // Escape always returns to View mode from any mode
    if keyboard.just_pressed(KeyCode::Escape) {
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
            EditorMode::Insert => {}
        }
        return;
    }

    // Transform operations only in Edit mode
    // Q = Translate, W = Rotate, R = Place
    if *current_mode.get() == EditorMode::Edit {
        if keyboard.just_pressed(KeyCode::KeyQ) {
            *transform_op = TransformOperation::Translate;
            *axis_constraint = AxisConstraint::None;
        } else if keyboard.just_pressed(KeyCode::KeyW) {
            *transform_op = TransformOperation::Rotate;
            *axis_constraint = AxisConstraint::None;
        } else if keyboard.just_pressed(KeyCode::KeyR) {
            *transform_op = TransformOperation::Place;
            *axis_constraint = AxisConstraint::None;
        }
        // Axis selection (A, S, D) is handled in gizmos/transform.rs
    }
}

/// Handle G key to group selected entities
fn handle_group_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut group_events: MessageWriter<GroupSelectedEvent>,
    mut contexts: EguiContexts,
) {
    // Don't handle when UI wants keyboard input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }

    // G to group selected entities
    if keyboard.just_pressed(KeyCode::KeyG) {
        group_events.write(GroupSelectedEvent);
    }
}

/// Handle P key to toggle preview mode
fn handle_preview_mode_shortcut(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut preview_events: MessageWriter<TogglePreviewModeEvent>,
    mut contexts: EguiContexts,
) {
    // Don't handle when UI wants keyboard input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }

    // P to toggle preview mode
    if keyboard.just_pressed(KeyCode::KeyP) {
        preview_events.write(TogglePreviewModeEvent);
    }
}
