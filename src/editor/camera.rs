use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::prelude::*;
use bevy_egui::EguiContexts;

use super::{EditorMode, EditorState};
use crate::selection::Selected;
use crate::ui::Settings;

/// Minimum FOV before switching to orthographic (in degrees)
const MIN_FOV_DEGREES: f32 = 5.0;
/// Maximum FOV (in degrees)
const MAX_FOV_DEGREES: f32 = 120.0;
/// FOV change per scroll unit
const FOV_SCROLL_SPEED: f32 = 5.0;
/// Orthographic scale when in ortho mode
const ORTHO_SCALE: f32 = 10.0;

pub struct EditorCameraPlugin;

/// Distance from target when looking at an object
const LOOK_AT_DISTANCE: f32 = 25.0;

impl Plugin for EditorCameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SetCameraPresetEvent>()
            .add_systems(Startup, spawn_editor_camera)
            .add_systems(Update, (camera_look, camera_movement, camera_zoom, handle_camera_preset, look_at_selected));
    }
}

/// Marker component for the editor camera
#[derive(Component)]
pub struct EditorCamera;

/// Fly camera state
#[derive(Component)]
pub struct FlyCamera {
    pub yaw: f32,
    pub pitch: f32,
    pub speed: f32,
    pub sensitivity: f32,
    /// Current FOV in degrees (0 = orthographic)
    pub fov_degrees: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: -std::f32::consts::FRAC_PI_6, // Look slightly down
            speed: 10.0,
            sensitivity: 0.003,
            fov_degrees: 60.0, // Default perspective FOV
        }
    }
}

/// Preset camera perspectives
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CameraPreset {
    Top,
    Bottom,
    Front,
    Back,
    Left,
    Right,
}

/// Default distance from origin for preset views
const PRESET_DISTANCE: f32 = 10.0;

impl CameraPreset {
    /// Get yaw and pitch for this preset
    pub fn angles(&self) -> (f32, f32) {
        use std::f32::consts::{FRAC_PI_2, PI};
        match self {
            CameraPreset::Front => (0.0, 0.0),
            CameraPreset::Back => (PI, 0.0),
            CameraPreset::Left => (FRAC_PI_2, 0.0),
            CameraPreset::Right => (-FRAC_PI_2, 0.0),
            CameraPreset::Top => (0.0, -FRAC_PI_2 + 0.001),
            CameraPreset::Bottom => (0.0, FRAC_PI_2 - 0.001),
        }
    }

    /// Get the camera position for this preset (looking at origin)
    pub fn position(&self) -> Vec3 {
        match self {
            CameraPreset::Front => Vec3::new(0.0, 0.0, PRESET_DISTANCE),
            CameraPreset::Back => Vec3::new(0.0, 0.0, -PRESET_DISTANCE),
            CameraPreset::Left => Vec3::new(-PRESET_DISTANCE, 0.0, 0.0),
            CameraPreset::Right => Vec3::new(PRESET_DISTANCE, 0.0, 0.0),
            CameraPreset::Top => Vec3::new(0.0, PRESET_DISTANCE, 0.0),
            CameraPreset::Bottom => Vec3::new(0.0, -PRESET_DISTANCE, 0.0),
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            CameraPreset::Top => "Top",
            CameraPreset::Bottom => "Bottom",
            CameraPreset::Front => "Front",
            CameraPreset::Back => "Back",
            CameraPreset::Left => "Left",
            CameraPreset::Right => "Right",
        }
    }
}

/// Event to set camera to a preset view
#[derive(Message)]
pub struct SetCameraPresetEvent(pub CameraPreset);

fn spawn_editor_camera(mut commands: Commands) {
    let fly_cam = FlyCamera::default();
    let rotation = Quat::from_euler(EulerRot::YXZ, fly_cam.yaw, fly_cam.pitch, 0.0);

    commands.spawn((
        EditorCamera,
        fly_cam,
        Camera3d::default(),
        Transform::from_translation(Vec3::new(0.0, 5.0, 10.0)).with_rotation(rotation),
    ));
}

/// Look around with right mouse button drag
fn camera_look(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    settings: Res<Settings>,
    mode: Res<State<EditorMode>>,
    mut query: Query<(&mut FlyCamera, &mut Transform), With<EditorCamera>>,
    mut contexts: EguiContexts,
) {
    // Disable freelook in Edit mode
    if *mode.get() == EditorMode::Edit {
        return;
    }

    // Don't capture mouse when UI wants pointer input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    if !mouse_button.pressed(MouseButton::Right) {
        return;
    }

    let delta = mouse_motion.delta;
    if delta == Vec2::ZERO {
        return;
    }

    for (mut fly_cam, mut transform) in &mut query {
        fly_cam.yaw -= delta.x * settings.camera_sensitivity;
        fly_cam.pitch = (fly_cam.pitch - delta.y * settings.camera_sensitivity)
            .clamp(-std::f32::consts::FRAC_PI_2 + 0.1, std::f32::consts::FRAC_PI_2 - 0.1);

        transform.rotation = Quat::from_euler(EulerRot::YXZ, fly_cam.yaw, fly_cam.pitch, 0.0);
    }
}

/// WASD movement for fly camera
fn camera_movement(
    keyboard: Res<ButtonInput<KeyCode>>,
    time: Res<Time>,
    settings: Res<Settings>,
    mode: Res<State<EditorMode>>,
    editor_state: Res<EditorState>,
    mut query: Query<&mut Transform, With<EditorCamera>>,
    mut contexts: EguiContexts,
) {
    // Don't handle when editor is disabled
    if !editor_state.editor_active {
        return;
    }

    // Disable camera movement in Edit mode (WASD used for axis selection)
    if *mode.get() == EditorMode::Edit {
        return;
    }

    // Don't move camera when UI wants keyboard input (e.g., text fields)
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }

    for mut transform in &mut query {
        let mut velocity = Vec3::ZERO;

        // Get forward/right/up vectors relative to camera orientation
        let forward = transform.forward().as_vec3();
        let right = transform.right().as_vec3();
        let up = transform.up().as_vec3();

        // WASD movement
        if keyboard.pressed(KeyCode::KeyW) {
            velocity += forward;
        }
        if keyboard.pressed(KeyCode::KeyS) {
            velocity -= forward;
        }
        if keyboard.pressed(KeyCode::KeyA) {
            velocity -= right;
        }
        if keyboard.pressed(KeyCode::KeyD) {
            velocity += right;
        }

        // Vertical movement with Space/Ctrl (relative to camera orientation)
        if keyboard.pressed(KeyCode::Space) {
            velocity += up;
        }
        if keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight) {
            velocity -= up;
        }

        if velocity != Vec3::ZERO {
            velocity = velocity.normalize();

            // Speed multiplier with Shift (faster)
            let speed_mult = if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
                3.0
            } else {
                1.0
            };

            transform.translation += velocity * settings.camera_speed * speed_mult * time.delta_secs();
        }
    }
}

/// Handle scroll wheel to adjust FOV / switch to orthographic
fn camera_zoom(
    scroll: Res<AccumulatedMouseScroll>,
    mut query: Query<(&mut FlyCamera, &mut Projection), With<EditorCamera>>,
    mut contexts: EguiContexts,
) {
    // Don't zoom when UI wants pointer input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    let scroll_y = scroll.delta.y;
    if scroll_y == 0.0 {
        return;
    }

    for (mut fly_cam, mut projection) in &mut query {
        // Scroll up = zoom in (decrease FOV), scroll down = zoom out (increase FOV)
        let new_fov = fly_cam.fov_degrees - scroll_y * FOV_SCROLL_SPEED;

        if new_fov <= 0.0 {
            // Switch to orthographic
            fly_cam.fov_degrees = 0.0;
            *projection = Projection::Orthographic(OrthographicProjection {
                scale: ORTHO_SCALE,
                ..OrthographicProjection::default_3d()
            });
        } else if new_fov >= MIN_FOV_DEGREES {
            // Perspective mode
            fly_cam.fov_degrees = new_fov.min(MAX_FOV_DEGREES);
            *projection = Projection::Perspective(PerspectiveProjection {
                fov: fly_cam.fov_degrees.to_radians(),
                ..default()
            });
        } else if fly_cam.fov_degrees == 0.0 && scroll_y < 0.0 {
            // Switching from ortho back to perspective
            fly_cam.fov_degrees = MIN_FOV_DEGREES;
            *projection = Projection::Perspective(PerspectiveProjection {
                fov: fly_cam.fov_degrees.to_radians(),
                ..default()
            });
        }
    }
}

/// Handle camera preset switching
fn handle_camera_preset(
    mut events: MessageReader<SetCameraPresetEvent>,
    mut query: Query<(&mut FlyCamera, &mut Transform), With<EditorCamera>>,
) {
    for event in events.read() {
        let preset = &event.0;
        let (yaw, pitch) = preset.angles();
        let position = preset.position();
        for (mut fly_cam, mut transform) in &mut query {
            fly_cam.yaw = yaw;
            fly_cam.pitch = pitch;
            transform.translation = position;
            transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
        }
    }
}

/// Look at the currently selected object when L is pressed
fn look_at_selected(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    selected_query: Query<&Transform, (With<Selected>, Without<EditorCamera>)>,
    mut camera_query: Query<(&mut FlyCamera, &mut Transform), With<EditorCamera>>,
    mut contexts: EguiContexts,
) {
    // Don't handle when editor is disabled
    if !editor_state.editor_active {
        return;
    }

    // Don't trigger when UI wants keyboard input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }

    if !keyboard.just_pressed(KeyCode::KeyL) {
        return;
    }

    // Get the selected entity's position
    let Ok(selected_transform) = selected_query.single() else {
        return;
    };
    let target = selected_transform.translation;

    for (mut fly_cam, mut camera_transform) in &mut camera_query {
        // 3/4 view offset: diagonal from above
        let offset = Vec3::new(1.0, 0.7, 1.0).normalize() * LOOK_AT_DISTANCE;
        let new_pos = target + offset;

        camera_transform.translation = new_pos;
        camera_transform.look_at(target, Vec3::Y);

        // Extract yaw and pitch from the resulting rotation
        let (yaw, pitch, _) = camera_transform.rotation.to_euler(EulerRot::YXZ);
        fly_cam.yaw = yaw;
        fly_cam.pitch = pitch;
    }
}
