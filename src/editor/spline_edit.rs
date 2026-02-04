//! Spline editing integration for the modal editor.
//!
//! This module bridges the bevy_spline_3d library with the modal editor,
//! providing spline control point editing when in Edit mode with a spline selected.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiContexts;
use bevy_spline_3d::prelude::*;

use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};

use super::state::{ControlPointSnapState, EditorMode, EditorState, SelectedControlPointIndex};
use crate::commands::TakeSnapshotCommand;
use crate::constants::physics;
use crate::editor::EditorCamera;
use crate::scene::SplineMarker;
use crate::selection::Selected;
use crate::utils::{should_process_input, snap_to_grid};

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
                // Apply grid snap to control points after library drag
                apply_control_point_grid_snap.run_if(in_state(EditorMode::Edit)),
                // Handle snap-to-object raycast for control points
                handle_control_point_snap_mode.run_if(in_state(EditorMode::Edit)),
                // Handle confirm/cancel for control point snap
                handle_control_point_snap_confirm.run_if(in_state(EditorMode::Edit)),
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
    snap_state: Res<ControlPointSnapState>,
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
    // Disable during snap-to-object so the library doesn't interfere
    let in_edit_with_spline = *mode.get() == EditorMode::Edit && !selected_splines.is_empty();
    spline_settings.enabled = in_edit_with_spline && !snap_state.active;

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
    mut snap_state: ResMut<ControlPointSnapState>,
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
        // Clear snap state when leaving edit mode
        snap_state.reset();
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
        // Clear snap state when spline is deselected
        snap_state.reset();
    }
}

/// Handle spline-specific hotkeys when in Edit mode with a spline selected.
fn handle_spline_hotkeys(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
    mut splines: Query<(Entity, &mut Spline), (With<Selected>, With<SplineMarker>)>,
    selected_points: Query<(Entity, &ControlPointMarker), With<SelectedControlPoint>>,
    all_markers: Query<(Entity, &ControlPointMarker)>,
    mut control_point_selection: ResMut<SelectedControlPointIndex>,
    mut snap_state: ResMut<ControlPointSnapState>,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Don't process spline hotkeys while right mouse is held (camera flight mode)
    if mouse_button.pressed(MouseButton::Right) {
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

    // S - Toggle closed/open
    if keyboard.just_pressed(KeyCode::KeyS) {
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

    // T - Snap control point to object surface
    if keyboard.just_pressed(KeyCode::KeyT) && !snap_state.active {
        // Need a selected control point
        let selected_index = control_point_selection.0.or_else(|| {
            selected_points.iter().next().map(|(_, m)| m.index)
        });

        if let Some(point_index) = selected_index {
            if let Some((entity, spline)) = splines.iter().next() {
                if point_index < spline.control_points.len() {
                    commands.queue(TakeSnapshotCommand {
                        description: "Snap control point to object".to_string(),
                    });
                    snap_state.active = true;
                    snap_state.spline_entity = Some(entity);
                    snap_state.point_index = Some(point_index);
                    snap_state.original_local_pos = Some(spline.control_points[point_index]);
                    info!("Control point snap mode: move cursor over surface and click to confirm");
                }
            }
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

/// Apply grid snapping to control points being dragged by the library's drag system.
///
/// Runs after the library drag so we can post-process positions.
/// Converts local → world, snaps each axis, then world → local.
fn apply_control_point_grid_snap(
    editor_state: Res<EditorState>,
    selection_state: Res<SelectionState>,
    mut splines: Query<(&mut Spline, &GlobalTransform), With<SplineMarker>>,
) {
    if editor_state.grid_snap <= 0.0 || !selection_state.dragging {
        return;
    }

    let grid = editor_state.grid_snap;

    for (spline_entity, point_index) in &selection_state.dragged_points {
        let Ok((mut spline, global_transform)) = splines.get_mut(*spline_entity) else {
            continue;
        };
        let Some(local_pos) = spline.control_points.get(*point_index).copied() else {
            continue;
        };

        // Convert local position to world space
        let world_pos = global_transform.transform_point(local_pos);

        // Snap each axis
        let snapped_world = Vec3::new(
            snap_to_grid(world_pos.x, grid),
            snap_to_grid(world_pos.y, grid),
            snap_to_grid(world_pos.z, grid),
        );

        // Convert back to local space
        let inverse = global_transform.affine().inverse();
        let snapped_local = inverse.transform_point3(snapped_world);

        spline.control_points[*point_index] = snapped_local;
    }
}

/// Handle snap-to-object raycast for a control point (T key snap mode).
///
/// When snap state is active, raycasts from cursor and moves the control point
/// to the hit position on surfaces.
fn handle_control_point_snap_mode(
    snap_state: Res<ControlPointSnapState>,
    camera_query: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    spatial_query: SpatialQuery,
    mut splines: Query<(Entity, &mut Spline, &GlobalTransform), With<SplineMarker>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut contexts: EguiContexts,
) {
    if !snap_state.active {
        return;
    }

    let Some(spline_entity) = snap_state.spline_entity else {
        return;
    };
    let Some(point_index) = snap_state.point_index else {
        return;
    };

    // Don't update when UI wants pointer input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    let Ok(window) = window_query.single() else {
        return;
    };

    let Some(cursor_position) = window.cursor_position() else {
        return;
    };

    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
        return;
    };

    // Exclude the spline entity from raycast
    let excluded: Vec<Entity> = vec![spline_entity];
    let filter = SpatialQueryFilter::default().with_excluded_entities(excluded);

    let Some(hit) = spatial_query.cast_ray(
        ray.origin,
        ray.direction,
        physics::RAYCAST_MAX_DISTANCE,
        true,
        &filter,
    ) else {
        return;
    };

    let hit_point = ray.origin + ray.direction * hit.distance;

    // Set the control point to the hit position (world → local)
    let Ok((_, mut spline, global_transform)) = splines.get_mut(spline_entity) else {
        return;
    };

    if point_index < spline.control_points.len() {
        let inverse = global_transform.affine().inverse();
        let local_pos = inverse.transform_point3(hit_point);
        spline.control_points[point_index] = local_pos;
    }
}

/// Handle confirm (left click) and cancel (Escape) for control point snap-to-object mode.
fn handle_control_point_snap_confirm(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut snap_state: ResMut<ControlPointSnapState>,
    mut splines: Query<&mut Spline, With<SplineMarker>>,
    mut contexts: EguiContexts,
) {
    if !snap_state.active {
        return;
    }

    // Escape: cancel and restore original position
    if keyboard.just_pressed(KeyCode::Escape) {
        if let (Some(spline_entity), Some(point_index), Some(original_pos)) =
            (snap_state.spline_entity, snap_state.point_index, snap_state.original_local_pos)
        {
            if let Ok(mut spline) = splines.get_mut(spline_entity) {
                if point_index < spline.control_points.len() {
                    spline.control_points[point_index] = original_pos;
                }
            }
        }
        snap_state.reset();
        info!("Control point snap cancelled");
        return;
    }

    // Left click: confirm
    if mouse_button.just_pressed(MouseButton::Left) {
        // Don't confirm if clicking on UI
        if let Ok(ctx) = contexts.ctx_mut() {
            if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
                return;
            }
        }

        snap_state.reset();
        info!("Control point snap confirmed");
    }
}
