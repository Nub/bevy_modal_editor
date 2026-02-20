use avian3d::prelude::{Collider, SimpleCollider, Sleeping, SleepingDisabled, SpatialQuery, SpatialQueryFilter};
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiContexts;

use bevy_spline_3d::prelude::Spline;

use crate::commands::TakeSnapshotCommand;
use crate::constants::physics;
use crate::editor::{ActiveEdgeSnaps, AxisConstraint, DimensionSnapSettings, EditStepAmount, EditorCamera, EditorMode, EditorState, GizmoAxisConstraint, SnapSubMode, TransformOperation};
use crate::gizmos::{SelectionCircleGizmo, XRayGizmoConfig, XRayGizmoDimmed};
use crate::scene::{Locked, SplineMarker};
use crate::selection::Selected;
use crate::ui::Settings;
use crate::utils::{get_half_height_along_normal_from_collider, should_process_input, snap_to_grid};

/// Default distance from camera when placing objects without hitting a surface
const PLACE_DEFAULT_DISTANCE: f32 = 10.0;

/// Marker component for entities currently being edited (to track sleep state)
#[derive(Component)]
pub struct BeingEdited;

/// Calculate world-space movement along an axis based on mouse delta in screen space.
/// Projects the axis to screen space and determines how much mouse movement should
/// translate to world movement, ensuring the object tracks the mouse 1:1 in screen space.
fn calculate_axis_movement(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    _window_query: &Query<&Window, With<PrimaryWindow>>,
    object_pos: Vec3,
    axis_dir: Vec3,
    mouse_delta: Vec2,
) -> f32 {
    // Project object position and a point 1 unit along the axis to screen space
    let Ok(screen_pos) = camera.world_to_viewport(camera_transform, object_pos) else {
        // Fallback to simple calculation if projection fails
        return (mouse_delta.x - mouse_delta.y) * 0.01;
    };

    let axis_point = object_pos + axis_dir;
    let Ok(screen_axis_pos) = camera.world_to_viewport(camera_transform, axis_point) else {
        return (mouse_delta.x - mouse_delta.y) * 0.01;
    };

    // Calculate the screen-space vector for 1 world unit along the axis
    let screen_axis_dir = screen_axis_pos - screen_pos;
    let screen_axis_len = screen_axis_dir.length();

    if screen_axis_len < 0.001 {
        // Axis is pointing directly at/away from camera, use fallback
        return -mouse_delta.y * 0.01;
    }

    let screen_axis_normalized = screen_axis_dir / screen_axis_len;

    // Project mouse delta onto the screen-space axis direction
    let projected_delta = mouse_delta.dot(screen_axis_normalized);

    // screen_axis_len = pixels per world unit along this axis
    // To move projected_delta pixels, we need projected_delta / screen_axis_len world units
    // This gives exact 1:1 tracking regardless of distance or FOV
    projected_delta / screen_axis_len
}

/// Calculate rotation amount based on mouse delta, accounting for camera orientation.
/// Projects mouse movement onto the screen-space tangent of the rotation circle
/// so that rotation direction and rate match what you see on screen.
fn calculate_rotation_amount(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    object_pos: Vec3,
    rotation_axis: Vec3,
    mouse_delta: Vec2,
) -> f32 {
    let Ok(screen_pos) = camera.world_to_viewport(camera_transform, object_pos) else {
        return (mouse_delta.x - mouse_delta.y) * 0.01;
    };

    let axis_point = object_pos + rotation_axis;
    let Ok(screen_axis_pos) = camera.world_to_viewport(camera_transform, axis_point) else {
        return (mouse_delta.x - mouse_delta.y) * 0.01;
    };

    let screen_axis_dir = screen_axis_pos - screen_pos;
    let screen_axis_len = screen_axis_dir.length();

    if screen_axis_len < 0.001 {
        // Axis points directly at/away from camera — rotation circle is fully visible
        // on screen. Fall back to horizontal mouse motion = rotation.
        return -mouse_delta.x * 0.01;
    }

    let screen_axis_normalized = screen_axis_dir / screen_axis_len;

    // The tangent to the rotation circle in screen space is perpendicular to the axis projection
    let screen_tangent = Vec2::new(-screen_axis_normalized.y, screen_axis_normalized.x);

    // Project mouse delta onto the tangent direction
    let projected_delta = mouse_delta.dot(screen_tangent);

    // Convert projected pixels to radians using the same scale as translation:
    // screen_axis_len = pixels per world unit, so this gives ~1 radian per world unit of movement
    projected_delta / screen_axis_len
}

/// Snap rotation (in radians) to nearest angle increment
fn snap_rotation(radians: f32, snap_degrees: f32) -> f32 {
    if snap_degrees <= 0.0 {
        radians
    } else {
        let snap_rad = snap_degrees.to_radians();
        (radians / snap_rad).round() * snap_rad
    }
}

/// Length of gizmo axes
const GIZMO_LENGTH: f32 = 1.5;

/// Click radius for gizmo axis detection (in world units, scaled by distance)
const GIZMO_CLICK_RADIUS: f32 = 0.15;

pub struct TransformGizmoPlugin;

impl Plugin for TransformGizmoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                draw_selection_gizmos,
                draw_distance_measurements,
                draw_edge_snap_guides,
                handle_gizmo_axis_click,
                handle_gizmo_axis_release,
                handle_axis_keys,
                handle_snap_submode_keys,
                handle_step_keys,
                handle_transform_manipulation,
                handle_place_mode,
                handle_place_mode_click,
                handle_snap_to_object_mode,
                handle_snap_to_object_click,
                manage_editing_sleep_state,
                clear_edge_snaps_when_idle,
            ),
        );
    }
}

/// Draw gizmos for selected entities
#[allow(clippy::too_many_arguments)]
fn draw_selection_gizmos(
    mut xray_gizmos: Gizmos<XRayGizmoConfig>,
    mut dimmed_gizmos: Gizmos<XRayGizmoDimmed>,
    mut sel_gizmos: Gizmos<SelectionCircleGizmo>,
    selected: Query<(&GlobalTransform, Option<&SplineMarker>, Option<&Spline>), With<Selected>>,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    axis_constraint: Res<AxisConstraint>,
    editor_state: Res<EditorState>,
    settings: Res<Settings>,
) {
    if !editor_state.gizmos_visible || *mode.get() == EditorMode::Particle {
        return;
    }

    for (global_transform, spline_marker, spline) in selected.iter() {
        let pos = global_transform.translation();

        // For splines, draw a thick selection highlight along the curve (splines don't have mesh outlines)
        if spline_marker.is_some() {
            if let Some(spline) = spline {
                draw_spline_selection_highlight(&mut sel_gizmos, spline, global_transform);
            }
        }
        // Note: Mesh-based entities use MeshOutline component for selection indication
        // (managed by sync_selection_outlines in selection.rs)

        // Draw transform gizmo in Edit mode using x-ray gizmos (always visible)
        let gizmo_scale = settings.gizmos.transform_scale;
        if *mode.get() == EditorMode::Edit {
            match *transform_op {
                TransformOperation::Translate => draw_translate_gizmo(&mut xray_gizmos, &mut dimmed_gizmos, pos, gizmo_scale, &axis_constraint),
                TransformOperation::Rotate => draw_rotate_gizmo(&mut xray_gizmos, &mut dimmed_gizmos, pos, gizmo_scale, &axis_constraint),
                TransformOperation::Scale => draw_scale_gizmo(&mut xray_gizmos, &mut dimmed_gizmos, pos, gizmo_scale, &axis_constraint),
                TransformOperation::Place | TransformOperation::SnapToObject | TransformOperation::None => {}
            }
        }
    }
}

/// Draw a thick selection highlight along the spline curve using the selection gizmo group
fn draw_spline_selection_highlight(gizmos: &mut Gizmos<SelectionCircleGizmo>, spline: &Spline, transform: &GlobalTransform) {
    if !spline.is_valid() {
        return;
    }

    // Sample the spline curve
    let points = spline.sample(32);
    if points.len() < 2 {
        return;
    }

    // Selection highlight color - orange to match other selection indicators
    let highlight_color = Color::srgb(1.0, 0.8, 0.0);

    // Draw the curve with selection highlight
    for i in 0..points.len() - 1 {
        let p0 = transform.transform_point(points[i]);
        let p1 = transform.transform_point(points[i + 1]);
        gizmos.line(p0, p1, highlight_color);
    }

    // If closed, draw the closing segment
    if spline.closed && points.len() >= 2 {
        let p0 = transform.transform_point(*points.last().unwrap());
        let p1 = transform.transform_point(points[0]);
        gizmos.line(p0, p1, highlight_color);
    }
}

/// Draw distance measurements between selected objects (View mode only)
fn draw_distance_measurements(
    mut gizmos: Gizmos,
    selected: Query<&GlobalTransform, With<Selected>>,
    editor_state: Res<EditorState>,
    mode: Res<State<EditorMode>>,
) {
    // Only show measurements in View mode
    if *mode.get() != EditorMode::View {
        return;
    }

    if !editor_state.gizmos_visible || !editor_state.measurements_visible {
        return;
    }

    // Collect positions of all selected entities
    let positions: Vec<Vec3> = selected.iter().map(|t| t.translation()).collect();

    // Only show measurements when 2 or more objects are selected
    if positions.len() < 2 {
        return;
    }

    // Draw distance lines between consecutive pairs (for simplicity)
    // For 2 objects: show distance between them
    // For 3+ objects: show distances forming a chain
    for i in 0..positions.len() - 1 {
        let start = positions[i];
        let end = positions[i + 1];
        let distance = start.distance(end);
        let midpoint = (start + end) * 0.5;

        // Draw a dashed line between objects
        let direction = (end - start).normalize_or_zero();
        let segments = 10;
        let segment_len = distance / segments as f32;

        for j in 0..segments {
            if j % 2 == 0 {
                let seg_start = start + direction * (j as f32 * segment_len);
                let seg_end = start + direction * ((j as f32 + 0.8) * segment_len);
                gizmos.line(seg_start, seg_end, Color::srgba(1.0, 1.0, 0.0, 0.7));
            }
        }

        // Draw small spheres at endpoints
        gizmos.sphere(Isometry3d::from_translation(start), 0.05, Color::srgba(1.0, 1.0, 0.0, 0.9));
        gizmos.sphere(Isometry3d::from_translation(end), 0.05, Color::srgba(1.0, 1.0, 0.0, 0.9));

        // Draw distance text indicator at midpoint (as a small cross)
        let text_size = 0.15;
        gizmos.line(
            midpoint - Vec3::X * text_size,
            midpoint + Vec3::X * text_size,
            Color::srgba(1.0, 1.0, 0.0, 1.0),
        );
        gizmos.line(
            midpoint - Vec3::Z * text_size,
            midpoint + Vec3::Z * text_size,
            Color::srgba(1.0, 1.0, 0.0, 1.0),
        );

    }
}

/// Handle clicking on gizmo axes to set axis constraint (only while held)
fn handle_gizmo_axis_click(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    editor_state: Res<EditorState>,
    settings: Res<Settings>,
    mut axis_constraint: ResMut<AxisConstraint>,
    mut gizmo_constraint: ResMut<GizmoAxisConstraint>,
    camera_query: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    selected: Query<&GlobalTransform, With<Selected>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut contexts: EguiContexts,
) {
    // Only handle in Edit mode with Translate, Rotate, or Scale operation
    if *mode.get() != EditorMode::Edit {
        return;
    }

    match *transform_op {
        TransformOperation::Translate | TransformOperation::Rotate | TransformOperation::Scale => {}
        _ => return,
    }

    // Only on left click press
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }

    // Don't handle when UI wants pointer input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    if !editor_state.gizmos_visible {
        return;
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

    // Create ray from camera through cursor position
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
        return;
    };

    let gizmo_scale = settings.gizmos.transform_scale;

    // Check each selected entity's gizmo
    for global_transform in selected.iter() {
        let gizmo_pos = global_transform.translation();
        let gizmo_length = gizmo_scale * GIZMO_LENGTH;

        // Calculate distance from ray to each axis
        let axes = [
            (Vec3::X, AxisConstraint::X),
            (Vec3::Y, AxisConstraint::Y),
            (Vec3::Z, AxisConstraint::Z),
        ];

        let mut closest_axis: Option<AxisConstraint> = None;
        let mut closest_distance = f32::MAX;

        for (axis_dir, constraint) in axes {
            let axis_end = gizmo_pos + axis_dir * gizmo_length;

            // Calculate distance from ray to line segment (gizmo_pos to axis_end)
            let distance = ray_to_line_segment_distance(
                ray.origin,
                ray.direction.into(),
                gizmo_pos,
                axis_end,
            );

            // Tighter click radius to reduce accidental clicks
            let camera_distance = (gizmo_pos - camera_transform.translation()).length();
            let click_radius = GIZMO_CLICK_RADIUS * gizmo_scale * 0.5 * (camera_distance / 5.0).max(1.0);

            if distance < click_radius && distance < closest_distance {
                closest_distance = distance;
                closest_axis = Some(constraint);
            }
        }

        // If we found an axis, set the constraint (will clear on release)
        if let Some(constraint) = closest_axis {
            *axis_constraint = constraint;
            gizmo_constraint.from_gizmo = true;
            return; // Only handle one gizmo click
        }
    }
}

/// Clear gizmo-based axis constraint when mouse is released
fn handle_gizmo_axis_release(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut axis_constraint: ResMut<AxisConstraint>,
    mut gizmo_constraint: ResMut<GizmoAxisConstraint>,
) {
    // Clear constraint when mouse released, if it was set via gizmo
    if mouse_button.just_released(MouseButton::Left) && gizmo_constraint.from_gizmo {
        *axis_constraint = AxisConstraint::None;
        gizmo_constraint.from_gizmo = false;
    }
}

/// Calculate the minimum distance from a ray to a line segment
fn ray_to_line_segment_distance(
    ray_origin: Vec3,
    ray_dir: Vec3,
    seg_start: Vec3,
    seg_end: Vec3,
) -> f32 {
    let seg_dir = seg_end - seg_start;
    let seg_len = seg_dir.length();

    if seg_len < 0.0001 {
        // Degenerate segment - just distance to point
        return point_to_ray_distance(seg_start, ray_origin, ray_dir);
    }

    let seg_dir_norm = seg_dir / seg_len;

    // Find closest points between ray and infinite line containing segment
    let w0 = ray_origin - seg_start;
    let a = ray_dir.dot(ray_dir);
    let b = ray_dir.dot(seg_dir_norm);
    let c = seg_dir_norm.dot(seg_dir_norm);
    let d = ray_dir.dot(w0);
    let e = seg_dir_norm.dot(w0);

    let denom = a * c - b * b;

    let (sc, tc) = if denom.abs() < 0.0001 {
        // Lines are nearly parallel
        (0.0, if b > c { d / b } else { e / c })
    } else {
        let sc = (b * e - c * d) / denom;
        let tc = (a * e - b * d) / denom;
        (sc.max(0.0), tc) // Ray only goes forward
    };

    // Clamp tc to segment bounds [0, seg_len]
    let tc_clamped = tc.clamp(0.0, seg_len);

    // Calculate closest points
    let closest_on_ray = ray_origin + ray_dir * sc;
    let closest_on_seg = seg_start + seg_dir_norm * tc_clamped;

    (closest_on_ray - closest_on_seg).length()
}

/// Calculate distance from a point to a ray
fn point_to_ray_distance(point: Vec3, ray_origin: Vec3, ray_dir: Vec3) -> f32 {
    let w = point - ray_origin;
    let c1 = w.dot(ray_dir);
    if c1 <= 0.0 {
        // Point is behind ray origin
        return w.length();
    }
    let c2 = ray_dir.dot(ray_dir);
    let b = c1 / c2;
    let closest = ray_origin + ray_dir * b;
    (point - closest).length()
}

/// Handle A/S/D keys to select X/Y/Z axis constraint in Edit mode
/// (A/S are handled separately in SnapToObject mode)
fn handle_axis_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    editor_state: Res<EditorState>,
    mut axis_constraint: ResMut<AxisConstraint>,
    mut contexts: EguiContexts,
) {
    // Only handle in Edit mode with an active transform operation
    if *mode.get() != EditorMode::Edit {
        return;
    }

    // Don't handle axis keys when right mouse is held (used for camera movement)
    if mouse_button.pressed(MouseButton::Right) {
        return;
    }

    if *transform_op == TransformOperation::None {
        return;
    }

    // In SnapToObject mode, A/S/D are used for sub-mode switching
    if *transform_op == TransformOperation::SnapToObject {
        return;
    }

    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // A = X axis, S = Y axis, D = Z axis
    if keyboard.just_pressed(KeyCode::KeyA) {
        *axis_constraint = if *axis_constraint == AxisConstraint::X {
            AxisConstraint::None
        } else {
            AxisConstraint::X
        };
    }
    if keyboard.just_pressed(KeyCode::KeyS) {
        *axis_constraint = if *axis_constraint == AxisConstraint::Y {
            AxisConstraint::None
        } else {
            AxisConstraint::Y
        };
    }
    if keyboard.just_pressed(KeyCode::KeyD) {
        *axis_constraint = if *axis_constraint == AxisConstraint::Z {
            AxisConstraint::None
        } else {
            AxisConstraint::Z
        };
    }
}

/// Handle scroll wheel to cycle snap sub-mode in SnapToObject mode
fn handle_snap_submode_keys(
    scroll: Res<AccumulatedMouseScroll>,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    editor_state: Res<EditorState>,
    mut snap_submode: ResMut<SnapSubMode>,
    mut contexts: EguiContexts,
) {
    // Only handle in Edit mode with SnapToObject operation
    if *mode.get() != EditorMode::Edit || *transform_op != TransformOperation::SnapToObject {
        return;
    }

    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // Don't handle when UI wants pointer input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    let scroll_y = scroll.delta.y;
    if scroll_y == 0.0 {
        return;
    }

    // Scroll up = next mode, scroll down = previous mode
    let new_mode = if scroll_y > 0.0 {
        match *snap_submode {
            SnapSubMode::Surface => SnapSubMode::Center,
            SnapSubMode::Center => SnapSubMode::Aligned,
            SnapSubMode::Aligned => SnapSubMode::Vertex,
            SnapSubMode::Vertex => SnapSubMode::Surface,
        }
    } else {
        match *snap_submode {
            SnapSubMode::Surface => SnapSubMode::Vertex,
            SnapSubMode::Center => SnapSubMode::Surface,
            SnapSubMode::Aligned => SnapSubMode::Center,
            SnapSubMode::Vertex => SnapSubMode::Aligned,
        }
    };

    *snap_submode = new_mode;
    let mode_name = match new_mode {
        SnapSubMode::Surface => "Surface",
        SnapSubMode::Center => "Center",
        SnapSubMode::Aligned => "Aligned",
        SnapSubMode::Vertex => "Vertex",
    };
    info!("Snap mode: {}", mode_name);
}

/// Handle J/K keys to decrease/increase transform values by step amount
fn handle_step_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    axis_constraint: Res<AxisConstraint>,
    step_amount: Res<EditStepAmount>,
    editor_state: Res<EditorState>,
    mut selected: Query<&mut Transform, (With<Selected>, Without<Locked>)>,
    mut contexts: EguiContexts,
    mut commands: Commands,
) {
    // Only handle in Edit mode with an active transform operation
    if *mode.get() != EditorMode::Edit {
        return;
    }

    if *transform_op == TransformOperation::None {
        return;
    }

    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // J = decrease, K = increase
    let direction = if keyboard.just_pressed(KeyCode::KeyK) {
        1.0
    } else if keyboard.just_pressed(KeyCode::KeyJ) {
        -1.0
    } else {
        return;
    };

    // Take snapshot before transforming
    if !selected.is_empty() {
        let op_name = match *transform_op {
            TransformOperation::Translate => "Move",
            TransformOperation::Rotate => "Rotate",
            TransformOperation::Scale => "Scale",
            _ => "Transform",
        };
        commands.queue(TakeSnapshotCommand {
            description: format!("{} entities (step)", op_name),
        });
    }

    for mut transform in selected.iter_mut() {
        match *transform_op {
            TransformOperation::Translate => {
                let delta = step_amount.translate * direction;
                match *axis_constraint {
                    AxisConstraint::None => {
                        transform.translation += Vec3::splat(delta);
                    }
                    AxisConstraint::X => transform.translation.x += delta,
                    AxisConstraint::Y => transform.translation.y += delta,
                    AxisConstraint::Z => transform.translation.z += delta,
                }
                // Apply grid snap if enabled
                if editor_state.grid_snap > 0.0 {
                    transform.translation.x = snap_to_grid(transform.translation.x, editor_state.grid_snap);
                    transform.translation.y = snap_to_grid(transform.translation.y, editor_state.grid_snap);
                    transform.translation.z = snap_to_grid(transform.translation.z, editor_state.grid_snap);
                }
            }
            TransformOperation::Rotate => {
                let delta_rad = step_amount.rotate.to_radians() * direction;
                let rotation = match *axis_constraint {
                    AxisConstraint::None | AxisConstraint::Y => Quat::from_rotation_y(delta_rad),
                    AxisConstraint::X => Quat::from_rotation_x(delta_rad),
                    AxisConstraint::Z => Quat::from_rotation_z(delta_rad),
                };
                transform.rotation = rotation * transform.rotation;
                // Apply rotation snap if enabled
                if editor_state.rotation_snap > 0.0 {
                    let (mut y, mut x, mut z) = transform.rotation.to_euler(EulerRot::YXZ);
                    x = snap_rotation(x, editor_state.rotation_snap);
                    y = snap_rotation(y, editor_state.rotation_snap);
                    z = snap_rotation(z, editor_state.rotation_snap);
                    transform.rotation = Quat::from_euler(EulerRot::YXZ, y, x, z);
                }
            }
            TransformOperation::Scale => {
                let delta = step_amount.scale * direction;
                match *axis_constraint {
                    AxisConstraint::None => {
                        transform.scale += Vec3::splat(delta);
                    }
                    AxisConstraint::X => transform.scale.x += delta,
                    AxisConstraint::Y => transform.scale.y += delta,
                    AxisConstraint::Z => transform.scale.z += delta,
                }
                transform.scale = transform.scale.max(Vec3::splat(0.01));
            }
            TransformOperation::Place | TransformOperation::SnapToObject | TransformOperation::None => {}
        }
    }
}

fn draw_translate_gizmo(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    dimmed: &mut Gizmos<XRayGizmoDimmed>,
    pos: Vec3,
    scale: f32,
    axis_constraint: &AxisConstraint,
) {
    let length = scale * GIZMO_LENGTH;
    let arrow_size = scale * 0.15;

    // Check which axes are active
    let x_active = *axis_constraint == AxisConstraint::X || *axis_constraint == AxisConstraint::None;
    let y_active = *axis_constraint == AxisConstraint::Y || *axis_constraint == AxisConstraint::None;
    let z_active = *axis_constraint == AxisConstraint::Z || *axis_constraint == AxisConstraint::None;

    // Full opacity colors for active axes
    let x_color = Color::srgb(1.0, 0.2, 0.2);
    let y_color = Color::srgb(0.2, 1.0, 0.2);
    let z_color = Color::srgb(0.2, 0.2, 1.0);

    // Half opacity colors for inactive axes
    let x_color_dim = Color::srgba(1.0, 0.2, 0.2, 0.5);
    let y_color_dim = Color::srgba(0.2, 1.0, 0.2, 0.5);
    let z_color_dim = Color::srgba(0.2, 0.2, 1.0, 0.5);

    // X axis (red)
    if x_active {
        gizmos.line(pos, pos + Vec3::X * length, x_color);
        gizmos.line(pos + Vec3::X * length, pos + Vec3::X * (length - arrow_size) + Vec3::Y * arrow_size, x_color);
        gizmos.line(pos + Vec3::X * length, pos + Vec3::X * (length - arrow_size) - Vec3::Y * arrow_size, x_color);
    } else {
        dimmed.line(pos, pos + Vec3::X * length, x_color_dim);
        dimmed.line(pos + Vec3::X * length, pos + Vec3::X * (length - arrow_size) + Vec3::Y * arrow_size, x_color_dim);
        dimmed.line(pos + Vec3::X * length, pos + Vec3::X * (length - arrow_size) - Vec3::Y * arrow_size, x_color_dim);
    }

    // Y axis (green)
    if y_active {
        gizmos.line(pos, pos + Vec3::Y * length, y_color);
        gizmos.line(pos + Vec3::Y * length, pos + Vec3::Y * (length - arrow_size) + Vec3::X * arrow_size, y_color);
        gizmos.line(pos + Vec3::Y * length, pos + Vec3::Y * (length - arrow_size) - Vec3::X * arrow_size, y_color);
    } else {
        dimmed.line(pos, pos + Vec3::Y * length, y_color_dim);
        dimmed.line(pos + Vec3::Y * length, pos + Vec3::Y * (length - arrow_size) + Vec3::X * arrow_size, y_color_dim);
        dimmed.line(pos + Vec3::Y * length, pos + Vec3::Y * (length - arrow_size) - Vec3::X * arrow_size, y_color_dim);
    }

    // Z axis (blue)
    if z_active {
        gizmos.line(pos, pos + Vec3::Z * length, z_color);
        gizmos.line(pos + Vec3::Z * length, pos + Vec3::Z * (length - arrow_size) + Vec3::Y * arrow_size, z_color);
        gizmos.line(pos + Vec3::Z * length, pos + Vec3::Z * (length - arrow_size) - Vec3::Y * arrow_size, z_color);
    } else {
        dimmed.line(pos, pos + Vec3::Z * length, z_color_dim);
        dimmed.line(pos + Vec3::Z * length, pos + Vec3::Z * (length - arrow_size) + Vec3::Y * arrow_size, z_color_dim);
        dimmed.line(pos + Vec3::Z * length, pos + Vec3::Z * (length - arrow_size) - Vec3::Y * arrow_size, z_color_dim);
    }
}

fn draw_rotate_gizmo(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    dimmed: &mut Gizmos<XRayGizmoDimmed>,
    pos: Vec3,
    scale: f32,
    axis_constraint: &AxisConstraint,
) {
    let radius = scale * 1.2;
    let segments = 32;

    let x_active = *axis_constraint == AxisConstraint::X || *axis_constraint == AxisConstraint::None;
    let y_active = *axis_constraint == AxisConstraint::Y || *axis_constraint == AxisConstraint::None;
    let z_active = *axis_constraint == AxisConstraint::Z || *axis_constraint == AxisConstraint::None;

    // Full opacity colors for active axes
    let x_color = Color::srgb(1.0, 0.2, 0.2);
    let y_color = Color::srgb(0.2, 1.0, 0.2);
    let z_color = Color::srgb(0.2, 0.2, 1.0);

    // Half opacity colors for inactive axes
    let x_color_dim = Color::srgba(1.0, 0.2, 0.2, 0.5);
    let y_color_dim = Color::srgba(0.2, 1.0, 0.2, 0.5);
    let z_color_dim = Color::srgba(0.2, 0.2, 1.0, 0.5);

    // X rotation (red circle in YZ plane)
    for i in 0..segments {
        let a1 = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let a2 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
        let p1 = pos + Vec3::new(0.0, a1.cos() * radius, a1.sin() * radius);
        let p2 = pos + Vec3::new(0.0, a2.cos() * radius, a2.sin() * radius);
        if x_active {
            gizmos.line(p1, p2, x_color);
        } else {
            dimmed.line(p1, p2, x_color_dim);
        }
    }

    // Y rotation (green circle in XZ plane)
    for i in 0..segments {
        let a1 = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let a2 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
        let p1 = pos + Vec3::new(a1.cos() * radius, 0.0, a1.sin() * radius);
        let p2 = pos + Vec3::new(a2.cos() * radius, 0.0, a2.sin() * radius);
        if y_active {
            gizmos.line(p1, p2, y_color);
        } else {
            dimmed.line(p1, p2, y_color_dim);
        }
    }

    // Z rotation (blue circle in XY plane)
    for i in 0..segments {
        let a1 = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let a2 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
        let p1 = pos + Vec3::new(a1.cos() * radius, a1.sin() * radius, 0.0);
        let p2 = pos + Vec3::new(a2.cos() * radius, a2.sin() * radius, 0.0);
        if z_active {
            gizmos.line(p1, p2, z_color);
        } else {
            dimmed.line(p1, p2, z_color_dim);
        }
    }
}

fn draw_scale_gizmo(
    gizmos: &mut Gizmos<XRayGizmoConfig>,
    dimmed: &mut Gizmos<XRayGizmoDimmed>,
    pos: Vec3,
    scale: f32,
    axis_constraint: &AxisConstraint,
) {
    let length = scale * 1.5;
    let box_size = scale * 0.1;

    let x_active = *axis_constraint == AxisConstraint::X || *axis_constraint == AxisConstraint::None;
    let y_active = *axis_constraint == AxisConstraint::Y || *axis_constraint == AxisConstraint::None;
    let z_active = *axis_constraint == AxisConstraint::Z || *axis_constraint == AxisConstraint::None;

    // Full opacity colors for active axes
    let x_color = Color::srgb(1.0, 0.2, 0.2);
    let y_color = Color::srgb(0.2, 1.0, 0.2);
    let z_color = Color::srgb(0.2, 0.2, 1.0);

    // Half opacity colors for inactive axes
    let x_color_dim = Color::srgba(1.0, 0.2, 0.2, 0.5);
    let y_color_dim = Color::srgba(0.2, 1.0, 0.2, 0.5);
    let z_color_dim = Color::srgba(0.2, 0.2, 1.0, 0.5);

    // X axis with box (red)
    if x_active {
        gizmos.line(pos, pos + Vec3::X * length, x_color);
        draw_small_cube(gizmos, pos + Vec3::X * length, box_size, x_color);
    } else {
        dimmed.line(pos, pos + Vec3::X * length, x_color_dim);
        draw_small_cube_dimmed(dimmed, pos + Vec3::X * length, box_size, x_color_dim);
    }

    // Y axis with box (green)
    if y_active {
        gizmos.line(pos, pos + Vec3::Y * length, y_color);
        draw_small_cube(gizmos, pos + Vec3::Y * length, box_size, y_color);
    } else {
        dimmed.line(pos, pos + Vec3::Y * length, y_color_dim);
        draw_small_cube_dimmed(dimmed, pos + Vec3::Y * length, box_size, y_color_dim);
    }

    // Z axis with box (blue)
    if z_active {
        gizmos.line(pos, pos + Vec3::Z * length, z_color);
        draw_small_cube(gizmos, pos + Vec3::Z * length, box_size, z_color);
    } else {
        dimmed.line(pos, pos + Vec3::Z * length, z_color_dim);
        draw_small_cube_dimmed(dimmed, pos + Vec3::Z * length, box_size, z_color_dim);
    }
}

fn draw_small_cube(gizmos: &mut Gizmos<XRayGizmoConfig>, pos: Vec3, size: f32, color: Color) {
    let half = size * 0.5;

    // Just draw the edges of a small cube
    let corners = [
        Vec3::new(-half, -half, -half),
        Vec3::new(half, -half, -half),
        Vec3::new(half, -half, half),
        Vec3::new(-half, -half, half),
        Vec3::new(-half, half, -half),
        Vec3::new(half, half, -half),
        Vec3::new(half, half, half),
        Vec3::new(-half, half, half),
    ];

    let edges = [
        (0, 1), (1, 2), (2, 3), (3, 0),
        (4, 5), (5, 6), (6, 7), (7, 4),
        (0, 4), (1, 5), (2, 6), (3, 7),
    ];

    for (a, b) in edges {
        gizmos.line(pos + corners[a], pos + corners[b], color);
    }
}

fn draw_small_cube_dimmed(gizmos: &mut Gizmos<XRayGizmoDimmed>, pos: Vec3, size: f32, color: Color) {
    let half = size * 0.5;

    let corners = [
        Vec3::new(-half, -half, -half),
        Vec3::new(half, -half, -half),
        Vec3::new(half, -half, half),
        Vec3::new(-half, -half, half),
        Vec3::new(-half, half, -half),
        Vec3::new(half, half, -half),
        Vec3::new(half, half, half),
        Vec3::new(-half, half, half),
    ];

    let edges = [
        (0, 1), (1, 2), (2, 3), (3, 0),
        (4, 5), (5, 6), (6, 7), (7, 4),
        (0, 4), (1, 5), (2, 6), (3, 7),
    ];

    for (a, b) in edges {
        gizmos.line(pos + corners[a], pos + corners[b], color);
    }
}

/// Handle mouse-based transform manipulation
#[allow(clippy::too_many_arguments)]
fn handle_transform_manipulation(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    axis_constraint: Res<AxisConstraint>,
    editor_state: Res<EditorState>,
    dimension_snap: Res<DimensionSnapSettings>,
    camera_query: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    mut selected: Query<(Entity, &mut Transform, Option<&Collider>), (With<Selected>, Without<Locked>)>,
    other_objects: Query<(Entity, &Transform, Option<&Collider>), (With<crate::scene::SceneEntity>, Without<Selected>)>,
    mut active_snaps: ResMut<ActiveEdgeSnaps>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut contexts: EguiContexts,
) {
    // Only manipulate in Edit mode with left mouse held
    if *mode.get() != EditorMode::Edit {
        return;
    }

    // Don't manipulate when UI wants pointer input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    if !mouse_button.pressed(MouseButton::Left) {
        return;
    }

    // Need an active transform operation (but not Place - that has its own handling)
    if *transform_op == TransformOperation::None || *transform_op == TransformOperation::Place {
        return;
    }

    let delta = mouse_motion.delta;
    if delta == Vec2::ZERO {
        return;
    }

    let Ok((camera, camera_transform)) = camera_query.single() else {
        return;
    };

    let camera_right = camera_transform.right();
    let camera_up = camera_transform.up();

    let sensitivity = 0.01;

    // Edge snapping is enabled by holding Alt while dragging
    let alt_held = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);

    // Collect nearby objects for edge snapping (only when translating with Alt held)
    let nearby_objects: Vec<(Vec3, Vec3)> = if alt_held && *transform_op == TransformOperation::Translate {
        other_objects
            .iter()
            .map(|(_, t, c)| {
                let half_extents = c
                    .map(|collider| {
                        let aabb = collider.aabb(Vec3::ZERO, Quat::IDENTITY);
                        aabb.size() * 0.5
                    })
                    .unwrap_or(Vec3::splat(0.5));
                (t.translation, half_extents)
            })
            .collect()
    } else {
        Vec::new()
    };

    // Clear previous snap lines
    active_snaps.snap_lines.clear();

    for (_entity, mut transform, collider) in selected.iter_mut() {
        match *transform_op {
            TransformOperation::Translate => {
                match *axis_constraint {
                    AxisConstraint::None => {
                        // Move in camera-relative plane
                        let move_right = camera_right * delta.x * sensitivity;
                        let move_up = camera_up * -delta.y * sensitivity;
                        transform.translation += move_right + move_up;
                    }
                    AxisConstraint::X | AxisConstraint::Y | AxisConstraint::Z => {
                        // Get the world-space axis direction
                        let axis_dir = match *axis_constraint {
                            AxisConstraint::X => Vec3::X,
                            AxisConstraint::Y => Vec3::Y,
                            AxisConstraint::Z => Vec3::Z,
                            AxisConstraint::None => unreachable!(),
                        };

                        // Project axis to screen space to determine how mouse movement maps to world movement
                        let movement = calculate_axis_movement(
                            camera,
                            camera_transform,
                            &window_query,
                            transform.translation,
                            axis_dir,
                            delta,
                        );
                        transform.translation += axis_dir * movement;
                    }
                }

                // Apply edge snapping if Alt is held (before grid snap)
                if alt_held && !nearby_objects.is_empty() {
                    let (snapped_pos, snap_lines) = calculate_edge_snaps(
                        transform.translation,
                        collider,
                        &nearby_objects,
                        dimension_snap.threshold,
                    );
                    transform.translation = snapped_pos;
                    active_snaps.snap_lines.extend(snap_lines);
                }

                // Apply grid snap if enabled
                if editor_state.grid_snap > 0.0 {
                    transform.translation.x = snap_to_grid(transform.translation.x, editor_state.grid_snap);
                    transform.translation.y = snap_to_grid(transform.translation.y, editor_state.grid_snap);
                    transform.translation.z = snap_to_grid(transform.translation.z, editor_state.grid_snap);
                }
            }
            TransformOperation::Rotate => {
                match *axis_constraint {
                    AxisConstraint::None => {
                        // Default to Y rotation (turntable) for unconstrained
                        let amount = calculate_rotation_amount(
                            camera,
                            camera_transform,
                            transform.translation,
                            Vec3::Y,
                            delta,
                        );
                        let rotation = Quat::from_rotation_y(amount);
                        transform.rotation = rotation * transform.rotation;
                    }
                    AxisConstraint::X | AxisConstraint::Y | AxisConstraint::Z => {
                        let axis = match *axis_constraint {
                            AxisConstraint::X => Vec3::X,
                            AxisConstraint::Y => Vec3::Y,
                            AxisConstraint::Z => Vec3::Z,
                            AxisConstraint::None => unreachable!(),
                        };

                        let amount = calculate_rotation_amount(
                            camera,
                            camera_transform,
                            transform.translation,
                            axis,
                            delta,
                        );
                        let rotation = Quat::from_axis_angle(axis, amount);
                        transform.rotation = rotation * transform.rotation;
                    }
                }
                // Apply rotation snap if enabled
                if editor_state.rotation_snap > 0.0 {
                    let (mut y, mut x, mut z) = transform.rotation.to_euler(EulerRot::YXZ);
                    x = snap_rotation(x, editor_state.rotation_snap);
                    y = snap_rotation(y, editor_state.rotation_snap);
                    z = snap_rotation(z, editor_state.rotation_snap);
                    transform.rotation = Quat::from_euler(EulerRot::YXZ, y, x, z);
                }
            }
            TransformOperation::Scale => {
                match *axis_constraint {
                    AxisConstraint::None => {
                        // Uniform scale: rightward mouse motion increases scale
                        let scale_factor = 1.0 + delta.x * sensitivity;
                        transform.scale *= scale_factor;
                    }
                    AxisConstraint::X | AxisConstraint::Y | AxisConstraint::Z => {
                        let axis_dir = match *axis_constraint {
                            AxisConstraint::X => Vec3::X,
                            AxisConstraint::Y => Vec3::Y,
                            AxisConstraint::Z => Vec3::Z,
                            AxisConstraint::None => unreachable!(),
                        };

                        // Project mouse delta onto screen-space axis direction.
                        // This ensures scale direction matches what you see on screen
                        // and the rate tracks the mouse 1:1.
                        let movement = calculate_axis_movement(
                            camera,
                            camera_transform,
                            &window_query,
                            transform.translation,
                            axis_dir,
                            delta,
                        );

                        match *axis_constraint {
                            AxisConstraint::X => transform.scale.x += movement,
                            AxisConstraint::Y => transform.scale.y += movement,
                            AxisConstraint::Z => transform.scale.z += movement,
                            AxisConstraint::None => unreachable!(),
                        }
                    }
                }
                transform.scale = transform.scale.clamp(Vec3::splat(0.1), Vec3::splat(100.0));
            }
            // Place mode is handled separately, None means no operation
            TransformOperation::Place | TransformOperation::SnapToObject | TransformOperation::None => {}
        }
    }
}

/// Manage sleeping state for entities being edited
/// Adds Sleeping + SleepingDisabled when editing starts, removes them when done
fn manage_editing_sleep_state(
    mut commands: Commands,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    selected: Query<Entity, (With<Selected>, Without<Locked>)>,
    being_edited: Query<Entity, With<BeingEdited>>,
    mut contexts: EguiContexts,
) {
    // Check if we should be in "editing" state
    let in_edit_mode = *mode.get() == EditorMode::Edit;
    let has_transform_op = *transform_op != TransformOperation::None
        && *transform_op != TransformOperation::Place
        && *transform_op != TransformOperation::SnapToObject;
    let mouse_held = mouse_button.pressed(MouseButton::Left);
    let mouse_just_pressed = mouse_button.just_pressed(MouseButton::Left);

    // Don't consider it editing if UI has pointer focus
    let ui_has_pointer = contexts
        .ctx_mut()
        .map(|ctx| ctx.wants_pointer_input() || ctx.is_pointer_over_area())
        .unwrap_or(false);

    let should_be_editing = in_edit_mode && has_transform_op && mouse_held && !ui_has_pointer;
    let just_started_editing = in_edit_mode && has_transform_op && mouse_just_pressed && !ui_has_pointer;

    // Take a snapshot when we first start editing
    if just_started_editing && !selected.is_empty() {
        let op_name = match *transform_op {
            TransformOperation::Translate => "Move",
            TransformOperation::Rotate => "Rotate",
            TransformOperation::Scale => "Scale",
            _ => "Transform",
        };
        info!("Taking transform snapshot: {} entities", op_name);
        commands.queue(TakeSnapshotCommand {
            description: format!("{} entities", op_name),
        });
    }

    if should_be_editing {
        // Start editing: add Sleeping and SleepingDisabled to selected entities
        for entity in selected.iter() {
            if !being_edited.contains(entity) {
                commands
                    .entity(entity)
                    .insert((BeingEdited, Sleeping, SleepingDisabled));
            }
        }
    } else {
        // Stop editing: remove sleep components and wake the bodies
        for entity in being_edited.iter() {
            commands
                .entity(entity)
                .remove::<(BeingEdited, Sleeping, SleepingDisabled)>();
        }
    }
}

/// Handle place mode - update selected entity positions based on raycast
fn handle_place_mode(
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    camera_query: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    spatial_query: SpatialQuery,
    mut selected: Query<(Entity, &mut Transform, Option<&Collider>), (With<Selected>, Without<Locked>)>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut contexts: EguiContexts,
) {
    // Only handle in Edit mode with Place operation
    if *mode.get() != EditorMode::Edit || *transform_op != TransformOperation::Place {
        return;
    }

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

    // Create ray from camera through cursor position
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
        return;
    };

    // Collect selected entities and their colliders for exclusion from raycast
    let selected_data: Vec<(Entity, Option<Collider>)> = selected
        .iter()
        .map(|(e, _, c)| (e, c.cloned()))
        .collect();

    if selected_data.is_empty() {
        return;
    }

    let selected_entities: Vec<Entity> = selected_data.iter().map(|(e, _)| *e).collect();

    // Cast ray against physics colliders (exclude selected entities)
    let filter = SpatialQueryFilter::default().with_excluded_entities(selected_entities);

    let position = if let Some(hit) = spatial_query.cast_ray(
        ray.origin,
        ray.direction,
        physics::RAYCAST_MAX_DISTANCE,
        true,
        &filter,
    ) {
        // Hit a surface - position on top of it
        let hit_point = ray.origin + ray.direction * hit.distance;
        let surface_normal = hit.normal;

        // Calculate half-height from the first selected entity's collider
        let half_height = selected_data
            .first()
            .and_then(|(_, c)| c.as_ref())
            .map(|collider| get_half_height_along_normal_from_collider(collider, surface_normal))
            .unwrap_or(0.5);

        // Offset along surface normal to place on top
        hit_point + surface_normal * half_height
    } else {
        // No hit - position at default distance from camera
        ray.origin + ray.direction * PLACE_DEFAULT_DISTANCE
    };

    // Move all selected entities to the new position
    // For multiple selections, maintain their relative positions
    if selected.iter().count() == 1 {
        // Single entity - move directly to position
        for (_, mut transform, _) in selected.iter_mut() {
            transform.translation = position;
        }
    } else {
        // Multiple entities - calculate center and move relative
        let center: Vec3 = selected.iter().map(|(_, t, _)| t.translation).sum::<Vec3>()
            / selected.iter().count() as f32;
        let offset = position - center;

        for (_, mut transform, _) in selected.iter_mut() {
            transform.translation += offset;
        }
    }
}

/// Handle click to confirm place mode placement
fn handle_place_mode_click(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mode: Res<State<EditorMode>>,
    mut transform_op: ResMut<TransformOperation>,
    mut contexts: EguiContexts,
) {
    // Only handle in Edit mode with Place operation
    if *mode.get() != EditorMode::Edit || *transform_op != TransformOperation::Place {
        return;
    }

    // Confirm on left click
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }

    // Don't confirm if clicking on UI
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    // Snapshot was already taken when entering place mode (R key)

    // Exit place mode
    *transform_op = TransformOperation::None;
    info!("Placement confirmed");
}

/// Handle snap to object mode - update position and rotation based on raycast
/// A key (Surface mode): Aligns the object so its local Y axis points along the surface normal
/// S key (Center mode): Aligns object centers through AABBs
fn handle_snap_to_object_mode(
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    snap_submode: Res<SnapSubMode>,
    camera_query: Query<(&Camera, &GlobalTransform), With<EditorCamera>>,
    spatial_query: SpatialQuery,
    mut selected: Query<(Entity, &mut Transform, Option<&Collider>), (With<Selected>, Without<Locked>)>,
    target_query: Query<(&Transform, Option<&Collider>), Without<Selected>>,
    window_query: Query<&Window, With<PrimaryWindow>>,
    mut contexts: EguiContexts,
) {
    // Only handle in Edit mode with SnapToObject operation
    if *mode.get() != EditorMode::Edit || *transform_op != TransformOperation::SnapToObject {
        return;
    }

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

    // Create ray from camera through cursor position
    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_position) else {
        return;
    };

    // Collect selected entities for exclusion from raycast
    let selected_entities: Vec<Entity> = selected.iter().map(|(e, _, _)| e).collect();

    if selected_entities.is_empty() {
        return;
    }

    // Cast ray against physics colliders (exclude selected entities)
    let filter = SpatialQueryFilter::default().with_excluded_entities(selected_entities);

    let Some(hit) = spatial_query.cast_ray(
        ray.origin,
        ray.direction,
        physics::RAYCAST_MAX_DISTANCE,
        true,
        &filter,
    ) else {
        // No hit - don't update position (keep object where it is)
        return;
    };

    let hit_point = ray.origin + ray.direction * hit.distance;
    let surface_normal = hit.normal.normalize();

    // Calculate half-height from the first selected entity's collider
    let selected_half_height = selected
        .iter()
        .next()
        .and_then(|(_, _, c)| c)
        .map(|collider| get_half_height_along_normal_from_collider(collider, surface_normal))
        .unwrap_or(0.5);

    match *snap_submode {
        SnapSubMode::Surface => {
            // Surface mode: align Y axis with surface normal
            let rotation = rotation_from_normal(surface_normal);
            let position = hit_point + surface_normal * selected_half_height;

            // Apply to all selected entities
            if selected.iter().count() == 1 {
                for (_, mut transform, _) in selected.iter_mut() {
                    transform.translation = position;
                    transform.rotation = rotation;
                }
            } else {
                let center: Vec3 = selected.iter().map(|(_, t, _)| t.translation).sum::<Vec3>()
                    / selected.iter().count() as f32;
                let offset = position - center;

                for (_, mut transform, _) in selected.iter_mut() {
                    transform.translation += offset;
                    transform.rotation = rotation;
                }
            }
        }
        SnapSubMode::Center => {
            // Center mode: align centers through AABBs
            // Get target entity info
            let Ok((target_transform, target_collider)) = target_query.get(hit.entity) else {
                return;
            };

            // Get target AABB half-extents (default to 0.5 if no collider)
            let target_half_extents = target_collider
                .map(|c| c.aabb(Vec3::ZERO, Quat::IDENTITY).size() * 0.5)
                .unwrap_or(Vec3::splat(0.5));

            // Get target center in world space
            let target_center = target_transform.translation;

            // Determine which axis the surface normal is most aligned with
            let abs_normal = surface_normal.abs();
            let primary_axis = if abs_normal.x >= abs_normal.y && abs_normal.x >= abs_normal.z {
                Vec3::X
            } else if abs_normal.y >= abs_normal.x && abs_normal.y >= abs_normal.z {
                Vec3::Y
            } else {
                Vec3::Z
            };

            // Calculate new positions for all selected entities
            // They will be placed adjacent to target, with centers aligned on the perpendicular axes
            for (_, mut transform, collider) in selected.iter_mut() {
                // Get selected entity's AABB half-extents
                let selected_half_extents = collider
                    .map(|c| c.aabb(Vec3::ZERO, Quat::IDENTITY).size() * 0.5)
                    .unwrap_or(Vec3::splat(0.5));

                // Position along the hit axis: target center + both half-extents along that axis
                let axis_offset = if surface_normal.dot(primary_axis) > 0.0 {
                    primary_axis
                } else {
                    -primary_axis
                };

                // Calculate distance from target center to selected center along the primary axis
                let distance_along_axis = if primary_axis == Vec3::X {
                    target_half_extents.x + selected_half_extents.x
                } else if primary_axis == Vec3::Y {
                    target_half_extents.y + selected_half_extents.y
                } else {
                    target_half_extents.z + selected_half_extents.z
                };

                // New position: aligned on perpendicular axes, offset on primary axis
                let mut new_pos = target_center;
                new_pos += axis_offset * distance_along_axis;

                transform.translation = new_pos;
                // Don't change rotation in center mode
            }
        }
        SnapSubMode::Aligned => {
            // Aligned mode: like center mode but uses target's rotation for off-axis objects
            // Get target entity info
            let Ok((target_transform, target_collider)) = target_query.get(hit.entity) else {
                return;
            };

            // Get target AABB half-extents (default to 0.5 if no collider)
            let target_half_extents = target_collider
                .map(|c| c.aabb(Vec3::ZERO, Quat::IDENTITY).size() * 0.5)
                .unwrap_or(Vec3::splat(0.5));

            // Get target center and rotation
            let target_center = target_transform.translation;
            let target_rotation = target_transform.rotation;

            // Transform surface normal into target's local space
            let local_normal = target_rotation.inverse() * surface_normal;

            // Determine which local axis the surface normal is most aligned with
            let abs_local_normal = local_normal.abs();
            let (local_axis, axis_idx) = if abs_local_normal.x >= abs_local_normal.y && abs_local_normal.x >= abs_local_normal.z {
                (Vec3::X, 0)
            } else if abs_local_normal.y >= abs_local_normal.x && abs_local_normal.y >= abs_local_normal.z {
                (Vec3::Y, 1)
            } else {
                (Vec3::Z, 2)
            };

            // Get the world-space direction for this local axis
            let world_axis = target_rotation * local_axis;
            let axis_sign = if local_normal.dot(local_axis) > 0.0 { 1.0 } else { -1.0 };

            // Calculate new positions for all selected entities
            for (_, mut transform, collider) in selected.iter_mut() {
                // Get selected entity's AABB half-extents
                let selected_half_extents = collider
                    .map(|c| c.aabb(Vec3::ZERO, Quat::IDENTITY).size() * 0.5)
                    .unwrap_or(Vec3::splat(0.5));

                // Get the half-extent along the primary axis
                let target_extent = match axis_idx {
                    0 => target_half_extents.x,
                    1 => target_half_extents.y,
                    _ => target_half_extents.z,
                };
                let selected_extent = match axis_idx {
                    0 => selected_half_extents.x,
                    1 => selected_half_extents.y,
                    _ => selected_half_extents.z,
                };

                // Position: target center + offset along the world-space axis
                let distance = target_extent + selected_extent;
                let new_pos = target_center + world_axis * axis_sign * distance;

                transform.translation = new_pos;
                // Match target's rotation
                transform.rotation = target_rotation;
            }
        }
        SnapSubMode::Vertex => {
            // Vertex mode: snap to nearest vertex of target mesh
            // For now, snap to the hit point (vertex snapping requires mesh data access)
            // This is a simplified implementation - just snap to the exact hit point
            let position = hit_point;

            // Apply to all selected entities
            if selected.iter().count() == 1 {
                for (_, mut transform, _) in selected.iter_mut() {
                    transform.translation = position;
                    // Keep current rotation in vertex mode
                }
            } else {
                let center: Vec3 = selected.iter().map(|(_, t, _)| t.translation).sum::<Vec3>()
                    / selected.iter().count() as f32;
                let offset = position - center;

                for (_, mut transform, _) in selected.iter_mut() {
                    transform.translation += offset;
                    // Keep current rotation
                }
            }
        }
    }
}

/// Calculate a rotation quaternion that aligns the local Y axis with the given normal
fn rotation_from_normal(normal: Vec3) -> Quat {
    let up = Vec3::Y;

    // Handle the case where normal is nearly parallel to Y axis
    if normal.dot(up).abs() > 0.999 {
        // Normal is nearly vertical
        if normal.y > 0.0 {
            // Pointing up - identity rotation
            Quat::IDENTITY
        } else {
            // Pointing down - rotate 180 degrees around X or Z
            Quat::from_rotation_x(std::f32::consts::PI)
        }
    } else {
        // General case: rotate from Y axis to the normal
        Quat::from_rotation_arc(up, normal)
    }
}

/// Handle click to confirm snap to object mode
fn handle_snap_to_object_click(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mode: Res<State<EditorMode>>,
    mut transform_op: ResMut<TransformOperation>,
    mut contexts: EguiContexts,
) {
    // Only handle in Edit mode with SnapToObject operation
    if *mode.get() != EditorMode::Edit || *transform_op != TransformOperation::SnapToObject {
        return;
    }

    // Confirm on left click
    if !mouse_button.just_pressed(MouseButton::Left) {
        return;
    }

    // Don't confirm if clicking on UI
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    // Snapshot was already taken when entering snap mode (T key)

    // Exit snap to object mode
    *transform_op = TransformOperation::None;
    info!("Snap to object confirmed");
}

/// Calculate edge snaps for a position based on nearby objects
/// Returns the snapped position and a list of snap guide lines
fn calculate_edge_snaps(
    position: Vec3,
    selected_collider: Option<&Collider>,
    nearby_objects: &[(Vec3, Vec3)], // (position, half_extents) of other objects
    threshold: f32,
) -> (Vec3, Vec<(Vec3, Vec3)>) {
    let mut snapped_pos = position;
    let mut snap_lines = Vec::new();

    // Get selected object's half extents (default to 0.5 cube)
    let selected_half = selected_collider
        .map(|c| {
            let aabb = c.aabb(Vec3::ZERO, Quat::IDENTITY);
            aabb.size() * 0.5
        })
        .unwrap_or(Vec3::splat(0.5));

    // Calculate selected object's AABB edges at current position
    let sel_min = position - selected_half;
    let sel_max = position + selected_half;

    // Check against each nearby object
    for (other_pos, other_half) in nearby_objects {
        let other_min = *other_pos - *other_half;
        let other_max = *other_pos + *other_half;

        // Check X axis edges
        // Alignment snaps first (lower priority), then adjacency snaps (higher priority).
        // When both trigger simultaneously, adjacency wins since it overwrites.

        // Selected min X vs Other min X (left to left alignment)
        if (sel_min.x - other_min.x).abs() < threshold {
            snapped_pos.x = other_min.x + selected_half.x;
            let snap_x = other_min.x;
            let y_min = sel_min.y.min(other_min.y);
            let y_max = sel_max.y.max(other_max.y);
            snap_lines.push((Vec3::new(snap_x, y_min, position.z), Vec3::new(snap_x, y_max, position.z)));
        }
        // Selected max X vs Other max X (right to right alignment)
        if (sel_max.x - other_max.x).abs() < threshold {
            snapped_pos.x = other_max.x - selected_half.x;
            let snap_x = other_max.x;
            let y_min = sel_min.y.min(other_min.y);
            let y_max = sel_max.y.max(other_max.y);
            snap_lines.push((Vec3::new(snap_x, y_min, position.z), Vec3::new(snap_x, y_max, position.z)));
        }
        // Selected min X vs Other max X (left edge to right edge — adjacency)
        if (sel_min.x - other_max.x).abs() < threshold {
            snapped_pos.x = other_max.x + selected_half.x;
            let snap_x = other_max.x;
            let y_min = sel_min.y.min(other_min.y);
            let y_max = sel_max.y.max(other_max.y);
            snap_lines.push((Vec3::new(snap_x, y_min, position.z), Vec3::new(snap_x, y_max, position.z)));
        }
        // Selected max X vs Other min X (right edge to left edge — adjacency)
        if (sel_max.x - other_min.x).abs() < threshold {
            snapped_pos.x = other_min.x - selected_half.x;
            let snap_x = other_min.x;
            let y_min = sel_min.y.min(other_min.y);
            let y_max = sel_max.y.max(other_max.y);
            snap_lines.push((Vec3::new(snap_x, y_min, position.z), Vec3::new(snap_x, y_max, position.z)));
        }

        // Check Z axis edges
        // Alignment first, then adjacency

        // Selected min Z vs Other min Z (alignment)
        if (sel_min.z - other_min.z).abs() < threshold {
            snapped_pos.z = other_min.z + selected_half.z;
            let snap_z = other_min.z;
            let y_min = sel_min.y.min(other_min.y);
            let y_max = sel_max.y.max(other_max.y);
            snap_lines.push((Vec3::new(position.x, y_min, snap_z), Vec3::new(position.x, y_max, snap_z)));
        }
        // Selected max Z vs Other max Z (alignment)
        if (sel_max.z - other_max.z).abs() < threshold {
            snapped_pos.z = other_max.z - selected_half.z;
            let snap_z = other_max.z;
            let y_min = sel_min.y.min(other_min.y);
            let y_max = sel_max.y.max(other_max.y);
            snap_lines.push((Vec3::new(position.x, y_min, snap_z), Vec3::new(position.x, y_max, snap_z)));
        }
        // Selected min Z vs Other max Z (adjacency)
        if (sel_min.z - other_max.z).abs() < threshold {
            snapped_pos.z = other_max.z + selected_half.z;
            let snap_z = other_max.z;
            let y_min = sel_min.y.min(other_min.y);
            let y_max = sel_max.y.max(other_max.y);
            snap_lines.push((Vec3::new(position.x, y_min, snap_z), Vec3::new(position.x, y_max, snap_z)));
        }
        // Selected max Z vs Other min Z (adjacency)
        if (sel_max.z - other_min.z).abs() < threshold {
            snapped_pos.z = other_min.z - selected_half.z;
            let snap_z = other_min.z;
            let y_min = sel_min.y.min(other_min.y);
            let y_max = sel_max.y.max(other_max.y);
            snap_lines.push((Vec3::new(position.x, y_min, snap_z), Vec3::new(position.x, y_max, snap_z)));
        }

        // Check Y axis edges
        // Alignment first, then adjacency

        // Selected min Y vs Other min Y (bottom alignment)
        if (sel_min.y - other_min.y).abs() < threshold {
            snapped_pos.y = other_min.y + selected_half.y;
            let snap_y = other_min.y;
            snap_lines.push((Vec3::new(position.x - 1.0, snap_y, position.z), Vec3::new(position.x + 1.0, snap_y, position.z)));
        }
        // Selected max Y vs Other max Y (top alignment)
        if (sel_max.y - other_max.y).abs() < threshold {
            snapped_pos.y = other_max.y - selected_half.y;
            let snap_y = other_max.y;
            snap_lines.push((Vec3::new(position.x - 1.0, snap_y, position.z), Vec3::new(position.x + 1.0, snap_y, position.z)));
        }
        // Selected min Y vs Other max Y (bottom to top — adjacency)
        if (sel_min.y - other_max.y).abs() < threshold {
            snapped_pos.y = other_max.y + selected_half.y;
            let snap_y = other_max.y;
            snap_lines.push((Vec3::new(position.x - 1.0, snap_y, position.z), Vec3::new(position.x + 1.0, snap_y, position.z)));
        }
        // Selected max Y vs Other min Y (top to bottom — adjacency)
        if (sel_max.y - other_min.y).abs() < threshold {
            snapped_pos.y = other_min.y - selected_half.y;
            let snap_y = other_min.y;
            snap_lines.push((Vec3::new(position.x - 1.0, snap_y, position.z), Vec3::new(position.x + 1.0, snap_y, position.z)));
        }
    }

    (snapped_pos, snap_lines)
}

/// Draw edge snap guide lines
fn draw_edge_snap_guides(
    mut gizmos: Gizmos,
    active_snaps: Res<ActiveEdgeSnaps>,
    editor_state: Res<EditorState>,
) {
    if !editor_state.gizmos_visible {
        return;
    }

    let snap_color = Color::srgba(0.0, 1.0, 1.0, 0.8); // Cyan

    for (start, end) in &active_snaps.snap_lines {
        gizmos.line(*start, *end, snap_color);
    }
}

/// Clear edge snaps when not actively translating with Alt held
fn clear_edge_snaps_when_idle(
    mouse_button: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    mut active_snaps: ResMut<ActiveEdgeSnaps>,
) {
    let alt_held = keyboard.pressed(KeyCode::AltLeft) || keyboard.pressed(KeyCode::AltRight);
    let should_clear = *mode.get() != EditorMode::Edit
        || *transform_op != TransformOperation::Translate
        || !mouse_button.pressed(MouseButton::Left)
        || !alt_held;

    if should_clear && !active_snaps.snap_lines.is_empty() {
        active_snaps.snap_lines.clear();
    }
}
