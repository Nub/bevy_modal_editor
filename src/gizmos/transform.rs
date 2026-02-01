use avian3d::prelude::{Sleeping, SleepingDisabled, SpatialQuery, SpatialQueryFilter};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_egui::EguiContexts;

use crate::editor::{AxisConstraint, EditStepAmount, EditorCamera, EditorMode, EditorState, TransformOperation};
use crate::scene::Locked;
use crate::selection::Selected;

/// Default distance from camera when placing objects without hitting a surface
const PLACE_DEFAULT_DISTANCE: f32 = 10.0;

/// Marker component for entities currently being edited (to track sleep state)
#[derive(Component)]
pub struct BeingEdited;

/// Snap a value to the nearest grid increment
fn snap_to_grid(value: f32, grid_size: f32) -> f32 {
    if grid_size <= 0.0 {
        value
    } else {
        (value / grid_size).round() * grid_size
    }
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

pub struct TransformGizmoPlugin;

impl Plugin for TransformGizmoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                draw_selection_gizmos,
                handle_axis_keys,
                handle_step_keys,
                handle_transform_manipulation,
                handle_place_mode,
                handle_place_mode_click,
                manage_editing_sleep_state,
            ),
        );
    }
}

/// Draw gizmos for selected entities
fn draw_selection_gizmos(
    mut gizmos: Gizmos,
    selected: Query<&GlobalTransform, With<Selected>>,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    axis_constraint: Res<AxisConstraint>,
    editor_state: Res<EditorState>,
) {
    if !editor_state.gizmos_visible {
        return;
    }

    for global_transform in selected.iter() {
        let pos = global_transform.translation();
        let scale = 1.0;

        // Draw selection outline
        draw_selection_box(&mut gizmos, pos, scale);

        // Draw transform gizmo in Edit mode
        if *mode.get() == EditorMode::Edit {
            match *transform_op {
                TransformOperation::Translate => draw_translate_gizmo(&mut gizmos, pos, scale, &axis_constraint),
                TransformOperation::Rotate => draw_rotate_gizmo(&mut gizmos, pos, scale, &axis_constraint),
                TransformOperation::Scale => draw_scale_gizmo(&mut gizmos, pos, scale, &axis_constraint),
                TransformOperation::Place | TransformOperation::None => {}
            }
        }
    }
}

/// Handle A/S/D keys to select X/Y/Z axis constraint in Edit mode
fn handle_axis_keys(
    keyboard: Res<ButtonInput<KeyCode>>,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    mut axis_constraint: ResMut<AxisConstraint>,
    mut contexts: EguiContexts,
) {
    // Only handle in Edit mode with an active transform operation
    if *mode.get() != EditorMode::Edit {
        return;
    }

    if *transform_op == TransformOperation::None {
        return;
    }

    // Don't handle when UI wants keyboard input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
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
) {
    // Only handle in Edit mode with an active transform operation
    if *mode.get() != EditorMode::Edit {
        return;
    }

    if *transform_op == TransformOperation::None {
        return;
    }

    // Don't handle when UI wants keyboard input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }

    // J = decrease, K = increase
    let direction = if keyboard.just_pressed(KeyCode::KeyK) {
        1.0
    } else if keyboard.just_pressed(KeyCode::KeyJ) {
        -1.0
    } else {
        return;
    };

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
            TransformOperation::Place | TransformOperation::None => {}
        }
    }
}

fn draw_selection_box(gizmos: &mut Gizmos, pos: Vec3, size: f32) {
    let half = size * 0.5;
    let color = Color::srgb(1.0, 0.8, 0.0);

    // Bottom face
    gizmos.line(
        pos + Vec3::new(-half, -half, -half),
        pos + Vec3::new(half, -half, -half),
        color,
    );
    gizmos.line(
        pos + Vec3::new(half, -half, -half),
        pos + Vec3::new(half, -half, half),
        color,
    );
    gizmos.line(
        pos + Vec3::new(half, -half, half),
        pos + Vec3::new(-half, -half, half),
        color,
    );
    gizmos.line(
        pos + Vec3::new(-half, -half, half),
        pos + Vec3::new(-half, -half, -half),
        color,
    );

    // Top face
    gizmos.line(
        pos + Vec3::new(-half, half, -half),
        pos + Vec3::new(half, half, -half),
        color,
    );
    gizmos.line(
        pos + Vec3::new(half, half, -half),
        pos + Vec3::new(half, half, half),
        color,
    );
    gizmos.line(
        pos + Vec3::new(half, half, half),
        pos + Vec3::new(-half, half, half),
        color,
    );
    gizmos.line(
        pos + Vec3::new(-half, half, half),
        pos + Vec3::new(-half, half, -half),
        color,
    );

    // Vertical edges
    gizmos.line(
        pos + Vec3::new(-half, -half, -half),
        pos + Vec3::new(-half, half, -half),
        color,
    );
    gizmos.line(
        pos + Vec3::new(half, -half, -half),
        pos + Vec3::new(half, half, -half),
        color,
    );
    gizmos.line(
        pos + Vec3::new(half, -half, half),
        pos + Vec3::new(half, half, half),
        color,
    );
    gizmos.line(
        pos + Vec3::new(-half, -half, half),
        pos + Vec3::new(-half, half, half),
        color,
    );
}

fn draw_translate_gizmo(gizmos: &mut Gizmos, pos: Vec3, scale: f32, axis_constraint: &AxisConstraint) {
    let length = scale * GIZMO_LENGTH;
    let arrow_size = scale * 0.15;

    // Determine colors based on axis constraint (active axis is brighter)
    let x_color = if *axis_constraint == AxisConstraint::X || *axis_constraint == AxisConstraint::None {
        Color::srgb(1.0, 0.2, 0.2)
    } else {
        Color::srgba(1.0, 0.2, 0.2, 0.3)
    };
    let y_color = if *axis_constraint == AxisConstraint::Y || *axis_constraint == AxisConstraint::None {
        Color::srgb(0.2, 1.0, 0.2)
    } else {
        Color::srgba(0.2, 1.0, 0.2, 0.3)
    };
    let z_color = if *axis_constraint == AxisConstraint::Z || *axis_constraint == AxisConstraint::None {
        Color::srgb(0.2, 0.2, 1.0)
    } else {
        Color::srgba(0.2, 0.2, 1.0, 0.3)
    };

    // X axis (red)
    gizmos.line(pos, pos + Vec3::X * length, x_color);
    gizmos.line(
        pos + Vec3::X * length,
        pos + Vec3::X * (length - arrow_size) + Vec3::Y * arrow_size,
        x_color,
    );
    gizmos.line(
        pos + Vec3::X * length,
        pos + Vec3::X * (length - arrow_size) - Vec3::Y * arrow_size,
        x_color,
    );

    // Y axis (green)
    gizmos.line(pos, pos + Vec3::Y * length, y_color);
    gizmos.line(
        pos + Vec3::Y * length,
        pos + Vec3::Y * (length - arrow_size) + Vec3::X * arrow_size,
        y_color,
    );
    gizmos.line(
        pos + Vec3::Y * length,
        pos + Vec3::Y * (length - arrow_size) - Vec3::X * arrow_size,
        y_color,
    );

    // Z axis (blue)
    gizmos.line(pos, pos + Vec3::Z * length, z_color);
    gizmos.line(
        pos + Vec3::Z * length,
        pos + Vec3::Z * (length - arrow_size) + Vec3::Y * arrow_size,
        z_color,
    );
    gizmos.line(
        pos + Vec3::Z * length,
        pos + Vec3::Z * (length - arrow_size) - Vec3::Y * arrow_size,
        z_color,
    );
}

fn draw_rotate_gizmo(gizmos: &mut Gizmos, pos: Vec3, scale: f32, axis_constraint: &AxisConstraint) {
    let radius = scale * 1.2;
    let segments = 32;

    // Determine colors based on axis constraint
    let x_color = if *axis_constraint == AxisConstraint::X || *axis_constraint == AxisConstraint::None {
        Color::srgb(1.0, 0.2, 0.2)
    } else {
        Color::srgba(1.0, 0.2, 0.2, 0.3)
    };
    let y_color = if *axis_constraint == AxisConstraint::Y || *axis_constraint == AxisConstraint::None {
        Color::srgb(0.2, 1.0, 0.2)
    } else {
        Color::srgba(0.2, 1.0, 0.2, 0.3)
    };
    let z_color = if *axis_constraint == AxisConstraint::Z || *axis_constraint == AxisConstraint::None {
        Color::srgb(0.2, 0.2, 1.0)
    } else {
        Color::srgba(0.2, 0.2, 1.0, 0.3)
    };

    // X rotation (red circle in YZ plane)
    for i in 0..segments {
        let a1 = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let a2 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
        gizmos.line(
            pos + Vec3::new(0.0, a1.cos() * radius, a1.sin() * radius),
            pos + Vec3::new(0.0, a2.cos() * radius, a2.sin() * radius),
            x_color,
        );
    }

    // Y rotation (green circle in XZ plane)
    for i in 0..segments {
        let a1 = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let a2 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
        gizmos.line(
            pos + Vec3::new(a1.cos() * radius, 0.0, a1.sin() * radius),
            pos + Vec3::new(a2.cos() * radius, 0.0, a2.sin() * radius),
            y_color,
        );
    }

    // Z rotation (blue circle in XY plane)
    for i in 0..segments {
        let a1 = (i as f32 / segments as f32) * std::f32::consts::TAU;
        let a2 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
        gizmos.line(
            pos + Vec3::new(a1.cos() * radius, a1.sin() * radius, 0.0),
            pos + Vec3::new(a2.cos() * radius, a2.sin() * radius, 0.0),
            z_color,
        );
    }
}

fn draw_scale_gizmo(gizmos: &mut Gizmos, pos: Vec3, scale: f32, axis_constraint: &AxisConstraint) {
    let length = scale * 1.5;
    let box_size = scale * 0.1;

    // Determine colors based on axis constraint
    let x_color = if *axis_constraint == AxisConstraint::X || *axis_constraint == AxisConstraint::None {
        Color::srgb(1.0, 0.2, 0.2)
    } else {
        Color::srgba(1.0, 0.2, 0.2, 0.3)
    };
    let y_color = if *axis_constraint == AxisConstraint::Y || *axis_constraint == AxisConstraint::None {
        Color::srgb(0.2, 1.0, 0.2)
    } else {
        Color::srgba(0.2, 1.0, 0.2, 0.3)
    };
    let z_color = if *axis_constraint == AxisConstraint::Z || *axis_constraint == AxisConstraint::None {
        Color::srgb(0.2, 0.2, 1.0)
    } else {
        Color::srgba(0.2, 0.2, 1.0, 0.3)
    };

    // X axis with box (red)
    gizmos.line(pos, pos + Vec3::X * length, x_color);
    draw_small_cube(gizmos, pos + Vec3::X * length, box_size, x_color);

    // Y axis with box (green)
    gizmos.line(pos, pos + Vec3::Y * length, y_color);
    draw_small_cube(gizmos, pos + Vec3::Y * length, box_size, y_color);

    // Z axis with box (blue)
    gizmos.line(pos, pos + Vec3::Z * length, z_color);
    draw_small_cube(gizmos, pos + Vec3::Z * length, box_size, z_color);
}

fn draw_small_cube(gizmos: &mut Gizmos, pos: Vec3, size: f32, color: Color) {
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

/// Handle mouse-based transform manipulation
fn handle_transform_manipulation(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    axis_constraint: Res<AxisConstraint>,
    editor_state: Res<EditorState>,
    camera_query: Query<&GlobalTransform, With<EditorCamera>>,
    mut selected: Query<&mut Transform, (With<Selected>, Without<Locked>)>,
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

    let Ok(camera_transform) = camera_query.single() else {
        return;
    };

    let camera_right = camera_transform.right();
    let camera_up = camera_transform.up();

    let sensitivity = 0.01;

    for mut transform in selected.iter_mut() {
        match *transform_op {
            TransformOperation::Translate => {
                match *axis_constraint {
                    AxisConstraint::None => {
                        // Move in camera-relative plane
                        let move_right = camera_right * delta.x * sensitivity;
                        let move_up = camera_up * -delta.y * sensitivity;
                        transform.translation += move_right + move_up;
                    }
                    AxisConstraint::X => {
                        let movement = (delta.x - delta.y) * sensitivity;
                        transform.translation.x += movement;
                    }
                    AxisConstraint::Y => {
                        let movement = -delta.y * sensitivity;
                        transform.translation.y += movement;
                    }
                    AxisConstraint::Z => {
                        let movement = (delta.x - delta.y) * sensitivity;
                        transform.translation.z += movement;
                    }
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
                        let rotation = Quat::from_rotation_y(-delta.x * sensitivity);
                        transform.rotation = rotation * transform.rotation;
                    }
                    AxisConstraint::X => {
                        let rotation = Quat::from_rotation_x(-delta.y * sensitivity);
                        transform.rotation = rotation * transform.rotation;
                    }
                    AxisConstraint::Y => {
                        let rotation = Quat::from_rotation_y(-delta.x * sensitivity);
                        transform.rotation = rotation * transform.rotation;
                    }
                    AxisConstraint::Z => {
                        let rotation = Quat::from_rotation_z(-delta.x * sensitivity);
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
                let scale_delta = (delta.x - delta.y) * sensitivity * 0.1;
                match *axis_constraint {
                    AxisConstraint::None => {
                        let scale_factor = 1.0 + scale_delta;
                        transform.scale *= scale_factor;
                    }
                    AxisConstraint::X => {
                        transform.scale.x *= 1.0 + scale_delta;
                    }
                    AxisConstraint::Y => {
                        transform.scale.y *= 1.0 + scale_delta;
                    }
                    AxisConstraint::Z => {
                        transform.scale.z *= 1.0 + scale_delta;
                    }
                }
                transform.scale = transform.scale.clamp(Vec3::splat(0.1), Vec3::splat(100.0));
            }
            // Place mode is handled separately, None means no operation
            TransformOperation::Place | TransformOperation::None => {}
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
    let has_transform_op = *transform_op != TransformOperation::None;
    let mouse_held = mouse_button.pressed(MouseButton::Left);

    // Don't consider it editing if UI has pointer focus
    let ui_has_pointer = contexts
        .ctx_mut()
        .map(|ctx| ctx.wants_pointer_input() || ctx.is_pointer_over_area())
        .unwrap_or(false);

    let should_be_editing = in_edit_mode && has_transform_op && mouse_held && !ui_has_pointer;

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
    mut selected: Query<(Entity, &mut Transform), (With<Selected>, Without<Locked>)>,
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

    // Collect selected entities for exclusion from raycast
    let selected_entities: Vec<Entity> = selected.iter().map(|(e, _)| e).collect();

    if selected_entities.is_empty() {
        return;
    }

    // Cast ray against physics colliders (exclude selected entities)
    let filter = SpatialQueryFilter::default().with_excluded_entities(selected_entities);

    let position = if let Some(hit) = spatial_query.cast_ray(
        ray.origin,
        ray.direction,
        100.0,
        true,
        &filter,
    ) {
        // Hit a surface - position on top of it
        let hit_point = ray.origin + ray.direction * hit.distance;
        let surface_normal = hit.normal;

        // Offset slightly along surface normal to place on top
        hit_point + surface_normal * 0.5
    } else {
        // No hit - position at default distance from camera
        ray.origin + ray.direction * PLACE_DEFAULT_DISTANCE
    };

    // Move all selected entities to the new position
    // For multiple selections, maintain their relative positions
    if selected.iter().count() == 1 {
        // Single entity - move directly to position
        for (_, mut transform) in selected.iter_mut() {
            transform.translation = position;
        }
    } else {
        // Multiple entities - calculate center and move relative
        let center: Vec3 = selected.iter().map(|(_, t)| t.translation).sum::<Vec3>()
            / selected.iter().count() as f32;
        let offset = position - center;

        for (_, mut transform) in selected.iter_mut() {
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

    // Exit place mode
    *transform_op = TransformOperation::None;
    info!("Placement confirmed");
}
