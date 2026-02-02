use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiContexts;

use crate::editor::EditorCamera;
use crate::scene::SceneEntity;

/// Marker component for selected entities
#[derive(Component, Default)]
pub struct Selected;

/// Resource to track multi-selection state
#[derive(Resource, Default)]
pub struct SelectionState {
    pub multi_select: bool,
}

pub struct SelectionSystemPlugin;

impl Plugin for SelectionSystemPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SelectionState>()
            .add_systems(Update, (update_multi_select_state, handle_click_selection));
    }
}

/// Track shift key for multi-selection
fn update_multi_select_state(keyboard: Res<ButtonInput<KeyCode>>, mut state: ResMut<SelectionState>) {
    state.multi_select = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
}

/// Handle click-to-select using raycasting
fn handle_click_selection(
    mouse_button: Res<ButtonInput<MouseButton>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    camera_query: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    spatial_query: SpatialQuery,
    scene_entities: Query<Entity, With<SceneEntity>>,
    selected: Query<Entity, With<Selected>>,
    selection_state: Res<SelectionState>,
    mut commands: Commands,
    mut contexts: EguiContexts,
) {
    // Only select on left click
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }

    // Don't process selection if clicking on UI
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    let Ok(window) = window_query.single() else {
        return;
    };

    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    // Create ray from camera through cursor
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
        return;
    };

    // Cast ray against physics colliders
    let filter = SpatialQueryFilter::default();
    if let Some(hit) = spatial_query.cast_ray(
        ray.origin,
        ray.direction,
        100.0,
        true,
        &filter,
    ) {
        let hit_entity = hit.entity;

        // Only select scene entities (locked items can still be selected)
        if scene_entities.get(hit_entity).is_ok() {
            if !selection_state.multi_select {
                // Clear previous selection
                for entity in selected.iter() {
                    commands.entity(entity).remove::<Selected>();
                }
            }

            // Toggle selection if multi-select and already selected
            if selection_state.multi_select && selected.get(hit_entity).is_ok() {
                commands.entity(hit_entity).remove::<Selected>();
            } else {
                commands.entity(hit_entity).insert(Selected);
            }
        }
    } else if !selection_state.multi_select {
        // Clicked on nothing - clear selection
        for entity in selected.iter() {
            commands.entity(entity).remove::<Selected>();
        }
    }
}
