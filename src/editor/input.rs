use bevy::prelude::*;
use bevy_egui::EguiContexts;

use super::state::{AxisConstraint, EditorMode, TransformOperation};
use crate::scene::GroupSelectedEvent;

pub struct EditorInputPlugin;

impl Plugin for EditorInputPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (handle_mode_input, handle_group_shortcut));
    }
}

/// Handle modal input switching and transform operation selection
fn handle_mode_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    current_mode: Res<State<EditorMode>>,
    mut next_mode: ResMut<NextState<EditorMode>>,
    mut transform_op: ResMut<TransformOperation>,
    mut axis_constraint: ResMut<AxisConstraint>,
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
            EditorMode::Edit => {
                next_mode.set(EditorMode::View);
                *transform_op = TransformOperation::None;
                *axis_constraint = AxisConstraint::None;
            }
        }
        return;
    }

    // Escape returns to View mode
    if keyboard.just_pressed(KeyCode::Escape) {
        if *current_mode.get() == EditorMode::Edit {
            next_mode.set(EditorMode::View);
            *transform_op = TransformOperation::None;
            *axis_constraint = AxisConstraint::None;
        }
        return;
    }

    // Transform operations only in Edit mode
    // Q = Translate, W = Rotate, E = Scale
    if *current_mode.get() == EditorMode::Edit {
        if keyboard.just_pressed(KeyCode::KeyQ) {
            *transform_op = TransformOperation::Translate;
            *axis_constraint = AxisConstraint::None;
        } else if keyboard.just_pressed(KeyCode::KeyW) {
            *transform_op = TransformOperation::Rotate;
            *axis_constraint = AxisConstraint::None;
        } else if keyboard.just_pressed(KeyCode::KeyE) {
            *transform_op = TransformOperation::Scale;
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
