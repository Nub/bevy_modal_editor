use bevy::prelude::*;

use crate::spline::{ControlPointMarker, SelectedControlPoint, SelectedSpline, Spline, SplineType};

use super::{selection::{clear_all_selections, SelectionState}, EditorSettings};

/// System to handle keyboard shortcuts for spline editing.
pub fn handle_hotkeys(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
    settings: Res<EditorSettings>,
    _selection_state: Res<SelectionState>,
    mut splines: Query<(Entity, &mut Spline), With<SelectedSpline>>,
    selected_points: Query<(Entity, &ControlPointMarker), With<SelectedControlPoint>>,
    all_markers: Query<(Entity, &ControlPointMarker)>,
) {
    if !settings.enabled || !settings.hotkeys_enabled {
        return;
    }

    // A - Add control point after selection
    if keyboard.just_pressed(KeyCode::KeyA) {
        handle_add_point(&mut commands, &mut splines, &selected_points);
    }

    // X - Delete selected control points
    if keyboard.just_pressed(KeyCode::KeyX) {
        handle_delete_points(&mut commands, &mut splines, &selected_points, &all_markers);
    }

    // Tab - Cycle spline type
    if keyboard.just_pressed(KeyCode::Tab) {
        for (_, mut spline) in &mut splines {
            spline.cycle_type();
        }
    }

    // C - Toggle closed/open
    if keyboard.just_pressed(KeyCode::KeyC) {
        for (_, mut spline) in &mut splines {
            spline.toggle_closed();
        }
    }

    // Escape - Deselect all
    if keyboard.just_pressed(KeyCode::Escape) {
        clear_all_selections(
            &mut commands,
            splines.iter().map(|(e, _)| e),
            selected_points.iter().map(|(e, _)| e),
        );
    }
}

fn handle_add_point(
    _commands: &mut Commands,
    splines: &mut Query<(Entity, &mut Spline), With<SelectedSpline>>,
    selected_points: &Query<(Entity, &ControlPointMarker), With<SelectedControlPoint>>,
) {
    // Find the highest selected index per spline
    let mut insert_after: std::collections::HashMap<Entity, usize> = std::collections::HashMap::new();

    for (_, marker) in selected_points.iter() {
        let entry = insert_after.entry(marker.spline_entity).or_insert(0);
        *entry = (*entry).max(marker.index);
    }

    for (entity, mut spline) in splines.iter_mut() {
        let insert_index = insert_after.get(&entity).copied().unwrap_or(
            spline.control_points.len().saturating_sub(1),
        );

        // Calculate new point position
        let new_pos = if spline.control_points.is_empty() {
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
        };

        // For Bézier splines, we need to add 3 points (handle, anchor, handle)
        if spline.spline_type == SplineType::CubicBezier {
            let idx = insert_index + 1;
            let offset = Vec3::new(0.3, 0.0, 0.0);
            spline.insert_point(idx, new_pos - offset); // Handle
            spline.insert_point(idx + 1, new_pos);      // Anchor
            spline.insert_point(idx + 2, new_pos + offset); // Handle
        } else {
            spline.insert_point(insert_index + 1, new_pos);
        }
    }
}

fn handle_delete_points(
    commands: &mut Commands,
    splines: &mut Query<(Entity, &mut Spline), With<SelectedSpline>>,
    selected_points: &Query<(Entity, &ControlPointMarker), With<SelectedControlPoint>>,
    _all_markers: &Query<(Entity, &ControlPointMarker)>,
) {
    // Group selected indices by spline, sorted in reverse order for deletion
    let mut to_delete: std::collections::HashMap<Entity, Vec<usize>> = std::collections::HashMap::new();

    for (_, marker) in selected_points.iter() {
        to_delete
            .entry(marker.spline_entity)
            .or_default()
            .push(marker.index);
    }

    for (entity, mut spline) in splines.iter_mut() {
        if let Some(indices) = to_delete.get(&entity) {
            let mut sorted_indices = indices.clone();
            sorted_indices.sort_unstable();
            sorted_indices.reverse();

            for index in sorted_indices {
                // Don't delete if it would leave too few points
                if spline.control_points.len() > spline.spline_type.min_points() {
                    spline.remove_point(index);
                }
            }
        }
    }

    // Clear selection on deleted points
    for (marker_entity, _) in selected_points.iter() {
        commands.entity(marker_entity).remove::<SelectedControlPoint>();
    }
}
