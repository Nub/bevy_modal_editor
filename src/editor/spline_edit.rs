//! Spline editing integration for the modal editor.
//!
//! This module bridges the bevy_spline_3d library with the modal editor,
//! providing spline control point editing when in Edit mode with a spline selected.

use bevy::prelude::*;
use bevy_egui::EguiContexts;
use bevy_spline_3d::prelude::*;

use super::state::{EditorMode, EditorState, SelectedControlPointIndex};
use crate::commands::TakeSnapshotCommand;
use crate::scene::SplineMarker;
use crate::selection::Selected;
use crate::utils::should_process_input;

pub struct SplineEditPlugin;

impl Plugin for SplineEditPlugin {
    fn build(&self, app: &mut App) {
        // Add the library's editor plugin for gizmo rendering
        app.add_plugins(SplineEditorPlugin);

        // Initialize our control point selection resource
        app.init_resource::<SelectedControlPointIndex>();

        app.add_systems(
            Update,
            (
                // Sync library's EditorSettings based on our editor state
                sync_spline_editor_settings,
                // Sync the library's SelectedSpline marker with our Selected component
                sync_spline_selection,
                // Handle spline-specific hotkeys in Edit mode
                handle_spline_hotkeys.run_if(in_state(EditorMode::Edit)),
                // Handle control point dragging in Edit mode
                handle_control_point_drag.run_if(in_state(EditorMode::Edit)),
            )
                .chain(),
        );
    }
}

/// Sync the library's EditorSettings with our modal editor state.
///
/// - Enable library input (picking/dragging) only in Edit mode with a spline selected
/// - Disable library hotkeys (we handle them ourselves with modal-aware input)
/// - Always show spline curves when editor is active
/// - Only show control points/handle lines when in Edit mode with a spline selected
fn sync_spline_editor_settings(
    editor_state: Res<EditorState>,
    mode: Res<State<EditorMode>>,
    selected_splines: Query<(), (With<Selected>, With<SplineMarker>)>,
    mut spline_settings: ResMut<EditorSettings>,
) {
    // Only show control points for the selected spline
    spline_settings.show_control_points_only_for_selected = true;

    // Disable the library's hotkey handling - we use our own modal-aware hotkeys
    spline_settings.hotkeys_enabled = false;

    // Don't clear selection when clicking on empty space - we manage selection externally
    spline_settings.clear_selection_on_empty_click = false;

    // Disable box selection - we use the editor's own selection system
    spline_settings.box_selection_enabled = false;

    // Always show spline curves when editor is active and gizmos are visible
    let should_show_curves = editor_state.editor_active && editor_state.gizmos_visible;

    // Enable control point picking/dragging only in Edit mode with a spline selected
    let in_edit_with_spline = *mode.get() == EditorMode::Edit && !selected_splines.is_empty();
    spline_settings.enabled = in_edit_with_spline;

    // Only enable x-ray when editing splines (so occluded control points are visible)
    spline_settings.xray_enabled = in_edit_with_spline;

    // Only show handle lines when in Edit mode with a spline selected
    let should_show_handles = should_show_curves && in_edit_with_spline;

    if spline_settings.show_gizmos != should_show_curves {
        spline_settings.show_gizmos = should_show_curves;
    }
    if spline_settings.show_handle_lines != should_show_handles {
        spline_settings.show_handle_lines = should_show_handles;
    }
}

/// Sync the library's SelectedSpline marker with our Selected component.
///
/// The library uses SelectedSpline to know which spline to show control points for.
/// We only add SelectedSpline when in Edit mode so control points only appear then.
fn sync_spline_selection(
    mut commands: Commands,
    mode: Res<State<EditorMode>>,
    selected_splines: Query<Entity, (With<Selected>, With<SplineMarker>, Without<SelectedSpline>)>,
    unselected_splines: Query<Entity, (Without<Selected>, With<SplineMarker>, With<SelectedSpline>)>,
    // Splines that have SelectedSpline but we're not in Edit mode
    splines_to_hide: Query<Entity, (With<SplineMarker>, With<SelectedSpline>)>,
    // All control point markers with selection
    selected_control_points: Query<(Entity, &ControlPointMarker), With<SelectedControlPoint>>,
    mut control_point_selection: ResMut<SelectedControlPointIndex>,
) {
    let in_edit_mode = *mode.get() == EditorMode::Edit;

    if in_edit_mode {
        // Add SelectedSpline to newly selected splines (only in Edit mode)
        for entity in &selected_splines {
            commands.entity(entity).insert(SelectedSpline);
        }
    } else {
        // Not in Edit mode - remove SelectedSpline from all splines to hide control points
        for entity in &splines_to_hide {
            commands.entity(entity).remove::<SelectedSpline>();
        }
        // Also clear all control point selections when leaving Edit mode
        for (entity, _) in &selected_control_points {
            commands.entity(entity).remove::<SelectedControlPoint>();
        }
        control_point_selection.0 = None;
    }

    // Remove SelectedSpline from deselected splines and clear their control point selections
    for spline_entity in &unselected_splines {
        commands.entity(spline_entity).remove::<SelectedSpline>();
        // Clear control point selections that belong to this spline
        for (marker_entity, marker) in &selected_control_points {
            if marker.spline_entity == spline_entity {
                commands.entity(marker_entity).remove::<SelectedControlPoint>();
            }
        }
        control_point_selection.0 = None;
    }
}

/// Handle spline-specific hotkeys when in Edit mode with a spline selected.
fn handle_spline_hotkeys(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
    mut splines: Query<(Entity, &mut Spline), (With<Selected>, With<SplineMarker>)>,
    selected_points: Query<(Entity, &ControlPointMarker), With<SelectedControlPoint>>,
    all_markers: Query<(Entity, &ControlPointMarker)>,
    mut control_point_selection: ResMut<SelectedControlPointIndex>,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Only handle if we have selected splines
    if splines.is_empty() {
        return;
    }

    // A - Add control point after selection
    if keyboard.just_pressed(KeyCode::KeyA) {
        commands.queue(TakeSnapshotCommand {
            description: "Add spline control point".to_string(),
        });
        for (entity, mut spline) in &mut splines {
            let insert_index = control_point_selection.0.unwrap_or(
                spline.control_points.len().saturating_sub(1),
            );

            let new_pos = calculate_new_point_position(&spline, insert_index);

            // For Bézier splines, add 3 points (handle, anchor, handle)
            if spline.spline_type == SplineType::CubicBezier {
                let idx = insert_index + 1;
                let offset = Vec3::new(0.3, 0.0, 0.0);
                spline.insert_point(idx, new_pos - offset);
                spline.insert_point(idx + 1, new_pos);
                spline.insert_point(idx + 2, new_pos + offset);
                // Select the new anchor point
                control_point_selection.0 = Some(idx + 1);
            } else {
                spline.insert_point(insert_index + 1, new_pos);
                control_point_selection.0 = Some(insert_index + 1);
            }

            info!("Added control point to spline {:?}", entity);
        }
    }

    // X - Delete selected control point
    if keyboard.just_pressed(KeyCode::KeyX) {
        commands.queue(TakeSnapshotCommand {
            description: "Delete spline control point".to_string(),
        });
        if let Some(selected_index) = control_point_selection.0 {
            for (entity, mut spline) in &mut splines {
                // Don't delete if it would leave too few points
                if spline.control_points.len() > spline.spline_type.min_points() {
                    spline.remove_point(selected_index);
                    // Clear selection after deletion
                    control_point_selection.0 = None;
                    // Also clear library's selection
                    for (marker_entity, marker) in &all_markers {
                        if marker.spline_entity == entity {
                            commands.entity(marker_entity).remove::<SelectedControlPoint>();
                        }
                    }
                    info!("Deleted control point {} from spline {:?}", selected_index, entity);
                }
            }
        } else {
            // If no control point selected, try to use library's selection
            for (entity, mut spline) in &mut splines {
                let mut indices_to_delete: Vec<usize> = selected_points
                    .iter()
                    .filter(|(_, m)| m.spline_entity == entity)
                    .map(|(_, m)| m.index)
                    .collect();

                indices_to_delete.sort_unstable();
                indices_to_delete.reverse();

                for index in indices_to_delete {
                    if spline.control_points.len() > spline.spline_type.min_points() {
                        spline.remove_point(index);
                    }
                }
            }
            // Clear library selection
            for (marker_entity, _) in &selected_points {
                commands.entity(marker_entity).remove::<SelectedControlPoint>();
            }
        }
    }

    // Tab - Cycle spline type (note: this conflicts with mode switching in View mode,
    // but we're in Edit mode here so it's safe)
    if keyboard.just_pressed(KeyCode::Tab) {
        commands.queue(TakeSnapshotCommand {
            description: "Cycle spline type".to_string(),
        });
        for (entity, mut spline) in &mut splines {
            spline.cycle_type();
            info!("Changed spline {:?} type to {:?}", entity, spline.spline_type);
        }
    }

    // C - Toggle closed/open
    if keyboard.just_pressed(KeyCode::KeyC) {
        commands.queue(TakeSnapshotCommand {
            description: "Toggle spline closed".to_string(),
        });
        for (entity, mut spline) in &mut splines {
            spline.toggle_closed();
            info!(
                "Spline {:?} is now {}",
                entity,
                if spline.closed { "closed" } else { "open" }
            );
        }
    }
}

/// Calculate the position for a new control point.
fn calculate_new_point_position(spline: &Spline, insert_index: usize) -> Vec3 {
    if spline.control_points.is_empty() {
        Vec3::ZERO
    } else if insert_index + 1 < spline.control_points.len() {
        // Midpoint between current and next
        (spline.control_points[insert_index] + spline.control_points[insert_index + 1]) / 2.0
    } else {
        // Extend in the direction of the spline
        let last = spline.control_points[insert_index];
        if insert_index > 0 {
            let prev = spline.control_points[insert_index - 1];
            last + (last - prev).normalize_or_zero() * 1.0
        } else {
            last + Vec3::X
        }
    }
}

/// Handle control point dragging in Edit mode.
///
/// This allows dragging control points when in Edit mode with a spline selected.
/// Also detects when a drag ends and takes an undo snapshot.
fn handle_control_point_drag(
    mut commands: Commands,
    mouse_button: Res<ButtonInput<MouseButton>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
    selected_splines: Query<(), (With<Selected>, With<SplineMarker>)>,
    mut control_point_selection: ResMut<SelectedControlPointIndex>,
    selected_points: Query<&ControlPointMarker, With<SelectedControlPoint>>,
    selection_state: Res<SelectionState>,
    mut was_dragging: Local<bool>,
) {
    // Detect drag start: take snapshot of the pre-drag state for undo
    if !*was_dragging && selection_state.dragging {
        commands.queue(TakeSnapshotCommand {
            description: "Move spline control point".to_string(),
        });
    }
    *was_dragging = selection_state.dragging;

    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Only handle if we have selected splines
    if selected_splines.is_empty() {
        return;
    }

    // Check if egui wants pointer input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    // Sync our control point selection with the library's selection
    // The library handles the actual picking and dragging via its own systems
    if !mouse_button.pressed(MouseButton::Left) {
        // Update our selection based on library's selection
        if let Some(marker) = selected_points.iter().next() {
            if control_point_selection.0 != Some(marker.index) {
                control_point_selection.0 = Some(marker.index);
            }
        }
    }
}
