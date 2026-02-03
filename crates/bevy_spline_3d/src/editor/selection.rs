use bevy::{prelude::*, window::PrimaryWindow};

use crate::spline::{
    get_effective_control_points, ControlPointMarker, ProjectedSplineCache, SelectedControlPoint,
    SelectedSpline, Spline,
};

use super::EditorSettings;

/// Resource tracking the current selection state.
#[derive(Resource, Default, Debug, Clone)]
pub struct SelectionState {
    /// Currently hovered control point, if any.
    pub hovered_point: Option<(Entity, usize)>,
    /// Whether we're currently dragging a point.
    pub dragging: bool,
    /// The point(s) being dragged: (spline_entity, point_index).
    pub dragged_points: Vec<(Entity, usize)>,
    /// The plane normal for drag operations (perpendicular to camera).
    pub drag_plane_normal: Vec3,
    /// The initial drag plane point (for consistent plane during drag).
    pub drag_plane_point: Vec3,
    /// Whether we're currently box selecting.
    pub box_selecting: bool,
    /// Screen-space start position of box selection.
    pub box_start: Vec2,
    /// Screen-space end position of box selection.
    pub box_end: Vec2,
}

/// Clear all spline and control point selections.
///
/// This is a helper function to reduce duplication in selection handling.
pub fn clear_all_selections(
    commands: &mut Commands,
    selected_splines: impl IntoIterator<Item = Entity>,
    selected_points: impl IntoIterator<Item = Entity>,
) {
    for entity in selected_splines {
        commands.entity(entity).remove::<SelectedSpline>();
    }
    for entity in selected_points {
        commands.entity(entity).remove::<SelectedControlPoint>();
    }
}

/// System to handle mouse picking of control points.
/// Uses projected positions when surface projection is enabled for the spline.
pub fn pick_control_points(
    settings: Res<EditorSettings>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    splines: Query<(Entity, &Spline, &GlobalTransform, Option<&ProjectedSplineCache>)>,
    mut selection_state: ResMut<SelectionState>,
) {
    if !settings.enabled {
        return;
    }

    // Don't update hover state while dragging
    if selection_state.dragging {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        selection_state.hovered_point = None;
        return;
    };

    // Find the active camera (handles multiple Camera3d entities)
    let Some((camera, camera_transform)) = cameras.iter().find(|(c, _)| c.is_active) else {
        return;
    };

    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
        return;
    };

    let mut closest: Option<(Entity, usize, f32)> = None;

    for (entity, spline, spline_transform, projected) in &splines {
        // Use the centralized helper to get effective control points
        let control_points = get_effective_control_points(spline, projected);

        for (i, &point) in control_points.iter().enumerate() {
            // Transform point to world space
            let world_point = spline_transform.transform_point(point);
            // Simple sphere-ray intersection
            let pick_radius = settings.sizes.point_radius * 2.0;
            if let Some(dist) = ray_sphere_intersect(ray.origin, ray.direction, world_point, pick_radius) {
                if closest.is_none() || dist < closest.unwrap().2 {
                    closest = Some((entity, i, dist));
                }
            }
        }
    }

    selection_state.hovered_point = closest.map(|(e, i, _)| (e, i));
}

fn ray_sphere_intersect(origin: Vec3, direction: Dir3, center: Vec3, radius: f32) -> Option<f32> {
    let oc = origin - center;
    let a = direction.dot(*direction);
    let b = 2.0 * oc.dot(*direction);
    let c = oc.dot(oc) - radius * radius;
    let discriminant = b * b - 4.0 * a * c;

    if discriminant < 0.0 {
        None
    } else {
        let t = (-b - discriminant.sqrt()) / (2.0 * a);
        if t > 0.0 {
            Some(t)
        } else {
            None
        }
    }
}

/// System to handle selection on mouse click.
pub fn handle_selection_click(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    settings: Res<EditorSettings>,
    selection_state: Res<SelectionState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    _splines: Query<(Entity, &Spline)>,
    _selected_splines: Query<Entity, With<SelectedSpline>>,
    markers: Query<(Entity, &ControlPointMarker)>,
    selected_points: Query<Entity, With<SelectedControlPoint>>,
    _cameras: Query<&GlobalTransform, With<Camera>>,
) {
    if !settings.enabled {
        return;
    }

    // Don't process clicks while dragging or box selecting
    if selection_state.dragging || selection_state.box_selecting {
        return;
    }

    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }

    let shift_held = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

    if let Some((spline_entity, point_index)) = selection_state.hovered_point {
        // Check if this point is already selected
        let already_selected = markers.iter().any(|(marker_entity, marker)| {
            marker.spline_entity == spline_entity
                && marker.index == point_index
                && selected_points.contains(marker_entity)
        });

        if already_selected {
            if shift_held {
                // Shift-click on selected point: deselect it
                for (marker_entity, marker) in &markers {
                    if marker.spline_entity == spline_entity && marker.index == point_index {
                        commands.entity(marker_entity).remove::<SelectedControlPoint>();
                    }
                }
            }
            // If not shift-held and already selected, do nothing -
            // this allows dragging multiple selected points without clearing selection
        } else {
            // Clicking on an unselected point
            if !shift_held {
                // Clear all control point selections when clicking without shift
                for entity in selected_points.iter() {
                    commands.entity(entity).remove::<SelectedControlPoint>();
                }
            }

            // Add selection to spline
            commands.entity(spline_entity).insert(SelectedSpline);

            // Find and select the control point marker
            for (marker_entity, marker) in &markers {
                if marker.spline_entity == spline_entity && marker.index == point_index {
                    commands.entity(marker_entity).insert(SelectedControlPoint);
                }
            }
        }
    }
    // Note: We don't clear selection on empty click here anymore.
    // Box selection handles that - if user just clicks without dragging,
    // selection is cleared when box selection ends with no points selected.
}

/// System to handle dragging control points.
/// When multiple points are selected, they all move together maintaining relative positions.
#[allow(clippy::too_many_arguments)]
pub fn handle_point_drag(
    mouse: Res<ButtonInput<MouseButton>>,
    settings: Res<EditorSettings>,
    mut selection_state: ResMut<SelectionState>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    mut splines: Query<(&mut Spline, &GlobalTransform, Option<&ProjectedSplineCache>)>,
    markers: Query<(Entity, &ControlPointMarker)>,
    selected_points: Query<Entity, With<SelectedControlPoint>>,
) {
    if !settings.enabled {
        return;
    }

    // Start drag - capture the hovered point and all selected points
    if mouse.just_pressed(MouseButton::Left) {
        if let Some((spline_entity, point_index)) = selection_state.hovered_point {
            // Check if the hovered point is already selected
            let hovered_is_selected = markers.iter().any(|(marker_entity, marker)| {
                marker.spline_entity == spline_entity
                    && marker.index == point_index
                    && selected_points.contains(marker_entity)
            });

            selection_state.dragging = true;

            if hovered_is_selected {
                // Drag all selected points together
                selection_state.dragged_points = markers
                    .iter()
                    .filter(|(marker_entity, _)| selected_points.contains(*marker_entity))
                    .map(|(_, marker)| (marker.spline_entity, marker.index))
                    .collect();
            } else {
                // Only drag the hovered point
                selection_state.dragged_points = vec![(spline_entity, point_index)];
            }

            if let Some((_, camera_transform)) = cameras.iter().find(|(c, _)| c.is_active) {
                selection_state.drag_plane_normal = camera_transform.forward().as_vec3();

                // Store initial plane point for consistent dragging (in world space)
                if let Ok((spline, spline_transform, projected)) = splines.get(spline_entity) {
                    let control_points = get_effective_control_points(spline, projected);
                    if let Some(&local_point) = control_points.get(point_index) {
                        // Transform to world space for the drag plane
                        selection_state.drag_plane_point = spline_transform.transform_point(local_point);
                    }
                }
            }
        }
    }

    // End drag
    if mouse.just_released(MouseButton::Left) {
        selection_state.dragging = false;
        selection_state.dragged_points.clear();
    }

    // Continue drag - move all dragged points by the same delta
    if selection_state.dragging && !selection_state.dragged_points.is_empty() {
        let Ok(window) = windows.single() else {
            return;
        };
        let Some(cursor_pos) = window.cursor_position() else {
            return;
        };
        let Some((camera, camera_transform)) = cameras.iter().find(|(c, _)| c.is_active) else {
            return;
        };
        let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
            return;
        };

        // Use the fixed drag plane point for consistent behavior (in world space)
        let plane_point = selection_state.drag_plane_point;
        let plane_normal = selection_state.drag_plane_normal;

        // Calculate the new position on the drag plane (in world space)
        let Some(new_world_pos) = ray_plane_intersect(
            ray.origin,
            *ray.direction,
            plane_point,
            plane_normal,
        ) else {
            return;
        };

        // Calculate delta in world space
        let world_delta = new_world_pos - plane_point;

        // Apply delta to all dragged points
        let dragged_points = selection_state.dragged_points.clone();

        // For single point drag, set position directly
        if dragged_points.len() == 1 {
            let (spline_entity, point_index) = dragged_points[0];
            if let Ok((mut spline, spline_transform, _)) = splines.get_mut(spline_entity) {
                if point_index < spline.control_points.len() {
                    // Convert world position to local space
                    let inverse_transform = spline_transform.affine().inverse();
                    let local_pos = inverse_transform.transform_point3(new_world_pos);
                    spline.control_points[point_index] = local_pos;
                }
            }
        } else {
            // For multi-point drag, apply delta to maintain relative positions
            for &(spline_entity, point_index) in &dragged_points {
                if let Ok((mut spline, spline_transform, _)) = splines.get_mut(spline_entity) {
                    if point_index < spline.control_points.len() {
                        // Convert world delta to local space delta
                        // We need to transform the delta direction, not position
                        let inverse_transform = spline_transform.affine().inverse();
                        let local_delta = inverse_transform.transform_vector3(world_delta);
                        spline.control_points[point_index] += local_delta;
                    }
                }
            }
            // Update the drag plane point so delta is computed correctly next frame
            selection_state.drag_plane_point = new_world_pos;
        }
    }
}

fn ray_plane_intersect(
    ray_origin: Vec3,
    ray_direction: Vec3,
    plane_point: Vec3,
    plane_normal: Vec3,
) -> Option<Vec3> {
    let denom = plane_normal.dot(ray_direction);
    if denom.abs() < 1e-6 {
        return None;
    }

    let t = (plane_point - ray_origin).dot(plane_normal) / denom;
    if t >= 0.0 {
        Some(ray_origin + ray_direction * t)
    } else {
        None
    }
}

/// System to handle box selection of multiple control points.
#[allow(clippy::too_many_arguments)]
pub fn handle_box_selection(
    mut commands: Commands,
    mouse: Res<ButtonInput<MouseButton>>,
    settings: Res<EditorSettings>,
    mut selection_state: ResMut<SelectionState>,
    keyboard: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    splines: Query<(Entity, &Spline, &GlobalTransform, Option<&ProjectedSplineCache>)>,
    markers: Query<(Entity, &ControlPointMarker)>,
    selected_splines: Query<Entity, With<SelectedSpline>>,
    selected_points: Query<Entity, With<SelectedControlPoint>>,
) {
    if !settings.enabled || !settings.box_selection_enabled {
        return;
    }

    // Don't box select while dragging a point
    if selection_state.dragging {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };

    let Some(cursor_pos) = window.cursor_position() else {
        return;
    };

    // Find the active camera (handles multiple Camera3d entities)
    let Some((camera, camera_transform)) = cameras.iter().find(|(c, _)| c.is_active) else {
        return;
    };

    let shift_held = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);

    // Start box selection when clicking on empty space
    if mouse.just_pressed(MouseButton::Left) && selection_state.hovered_point.is_none() {
        selection_state.box_selecting = true;
        selection_state.box_start = cursor_pos;
        selection_state.box_end = cursor_pos;

        // Always clear control point selection when starting box select (unless shift held)
        // Only clear spline selection if clear_selection_on_empty_click is enabled
        if !shift_held {
            // Always clear control point selections
            for entity in selected_points.iter() {
                commands.entity(entity).remove::<SelectedControlPoint>();
            }
            // Only clear spline selections if the setting allows
            if settings.clear_selection_on_empty_click {
                for entity in selected_splines.iter() {
                    commands.entity(entity).remove::<SelectedSpline>();
                }
            }
        }
    }

    // Update box end position while selecting
    if selection_state.box_selecting {
        selection_state.box_end = cursor_pos;
    }

    // End box selection
    if mouse.just_released(MouseButton::Left) && selection_state.box_selecting {
        selection_state.box_selecting = false;

        // Calculate box bounds (handle inverted boxes)
        let min_x = selection_state.box_start.x.min(selection_state.box_end.x);
        let max_x = selection_state.box_start.x.max(selection_state.box_end.x);
        let min_y = selection_state.box_start.y.min(selection_state.box_end.y);
        let max_y = selection_state.box_start.y.max(selection_state.box_end.y);

        // Only process if box has some size (not just a click)
        let box_size = (max_x - min_x) * (max_y - min_y);
        if box_size < 25.0 {
            // Too small, treat as a click on empty space - selection already cleared
            return;
        }

        // Find all control points within the box
        for (spline_entity, spline, spline_transform, projected) in &splines {
            let control_points = get_effective_control_points(spline, projected);

            for (point_index, &local_pos) in control_points.iter().enumerate() {
                // Transform to world space
                let world_pos = spline_transform.transform_point(local_pos);
                // Project world position to screen space
                let Ok(screen_pos) = camera.world_to_viewport(camera_transform, world_pos) else {
                    continue;
                };

                // Check if point is within box bounds
                if screen_pos.x >= min_x
                    && screen_pos.x <= max_x
                    && screen_pos.y >= min_y
                    && screen_pos.y <= max_y
                {
                    // Select this spline
                    commands.entity(spline_entity).insert(SelectedSpline);

                    // Find and select the control point marker
                    for (marker_entity, marker) in &markers {
                        if marker.spline_entity == spline_entity && marker.index == point_index {
                            commands.entity(marker_entity).insert(SelectedControlPoint);
                        }
                    }
                }
            }
        }
    }
}

/// Render the box selection rectangle.
pub fn render_box_selection(
    selection_state: Res<SelectionState>,
    settings: Res<EditorSettings>,
    mut gizmos: Gizmos,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
) {
    if !settings.enabled || !settings.show_gizmos || !settings.box_selection_enabled {
        return;
    }

    if !selection_state.box_selecting {
        return;
    }

    let Ok(_window) = windows.single() else {
        return;
    };

    // Find the active camera (handles multiple Camera3d entities)
    let Some((camera, camera_transform)) = cameras.iter().find(|(c, _)| c.is_active) else {
        return;
    };

    // Get the four corners of the box in screen space
    let start = selection_state.box_start;
    let end = selection_state.box_end;

    let corners_2d = [
        Vec2::new(start.x, start.y),
        Vec2::new(end.x, start.y),
        Vec2::new(end.x, end.y),
        Vec2::new(start.x, end.y),
    ];

    // Project corners to world space on a plane in front of the camera
    let cam_pos = camera_transform.translation();
    let cam_forward = camera_transform.forward();
    let plane_distance = 10.0; // Distance in front of camera to draw the box
    let plane_point = cam_pos + *cam_forward * plane_distance;

    let mut corners_3d = Vec::new();
    for corner_2d in &corners_2d {
        if let Ok(ray) = camera.viewport_to_world(camera_transform, *corner_2d) {
            if let Some(world_pos) = ray_plane_intersect(
                ray.origin,
                *ray.direction,
                plane_point,
                cam_forward.as_vec3(),
            ) {
                corners_3d.push(world_pos);
            }
        }
    }

    if corners_3d.len() == 4 {
        let color = Color::srgba(0.3, 0.6, 1.0, 0.8);
        gizmos.line(corners_3d[0], corners_3d[1], color);
        gizmos.line(corners_3d[1], corners_3d[2], color);
        gizmos.line(corners_3d[2], corners_3d[3], color);
        gizmos.line(corners_3d[3], corners_3d[0], color);
    }
}
