use avian3d::prelude::*;
use bevy::input::mouse::{AccumulatedMouseMotion, AccumulatedMouseScroll};
use bevy::light::VolumetricFog;
use bevy::prelude::*;
use bevy::render::view::Hdr;
use bevy_egui::{EguiContexts, EguiGlobalSettings, PrimaryEguiContext};
use bevy_outliner::prelude::*;

use bevy_editor_game::{GameCamera, GameState};

use super::{EditorMode, EditorState, TransformOperation};
use crate::selection::Selected;
use crate::ui::Settings;
use crate::utils::should_process_input;

/// Minimum FOV before switching to orthographic (in degrees)
const MIN_FOV_DEGREES: f32 = 30.0;
/// Maximum FOV (in degrees)
const MAX_FOV_DEGREES: f32 = 120.0;
/// FOV change per scroll unit
const FOV_SCROLL_SPEED: f32 = 5.0;
/// Default orthographic scale
const ORTHO_SCALE: f32 = 0.05;
/// Minimum orthographic scale (most zoomed in)
const MIN_ORTHO_SCALE: f32 = 0.001;
/// Maximum orthographic scale (most zoomed out)
const MAX_ORTHO_SCALE: f32 = 0.03;
/// Orthographic scale change multiplier per scroll unit
const ORTHO_ZOOM_SPEED: f32 = 0.1;

pub struct EditorCameraPlugin;

/// Minimum distance from target when framing objects
const MIN_FRAME_DISTANCE: f32 = 5.0;
/// Padding multiplier for framing (1.5 = 50% extra space around objects)
const FRAME_PADDING: f32 = 1.5;

impl Plugin for EditorCameraPlugin {
    fn build(&self, app: &mut App) {
        // Disable bevy_egui's auto context creation — we explicitly assign
        // PrimaryEguiContext to the editor camera to avoid it landing on
        // an outliner/cascade shadow camera instead.
        if let Some(mut settings) = app.world_mut().get_resource_mut::<EguiGlobalSettings>() {
            settings.auto_create_primary_context = false;
        }

        app.add_message::<SetCameraPresetEvent>()
            .add_systems(PreStartup, spawn_editor_camera)
            .add_systems(
                Update,
                (
                    camera_look,
                    camera_movement,
                    camera_zoom,
                    handle_camera_preset,
                    look_at_selected,
                    sync_camera_states,
                ),
            );
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
    /// Current orthographic scale (only used when fov_degrees == 0)
    pub ortho_scale: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: -std::f32::consts::FRAC_PI_6, // Look slightly down
            speed: 10.0,
            sensitivity: 0.003,
            fov_degrees: 60.0, // Default perspective FOV
            ortho_scale: ORTHO_SCALE,
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

/// Default distance from origin for perspective preset views
const PRESET_DISTANCE: f32 = 200.0;
/// Default distance from origin for orthographic preset views
const ORTHO_PRESET_DISTANCE: f32 = 10.0;

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
    pub fn position(&self, distance: f32) -> Vec3 {
        match self {
            CameraPreset::Front => Vec3::new(0.0, 0.0, distance),
            CameraPreset::Back => Vec3::new(0.0, 0.0, -distance),
            CameraPreset::Left => Vec3::new(-distance, 0.0, 0.0),
            CameraPreset::Right => Vec3::new(distance, 0.0, 0.0),
            CameraPreset::Top => Vec3::new(0.0, distance, 0.0),
            CameraPreset::Bottom => Vec3::new(0.0, -distance, 0.0),
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
pub struct SetCameraPresetEvent {
    pub preset: CameraPreset,
    /// If true, switch to orthographic projection
    pub orthographic: bool,
}

fn spawn_editor_camera(mut commands: Commands) {
    let fly_cam = FlyCamera::default();
    let rotation = Quat::from_euler(EulerRot::YXZ, fly_cam.yaw, fly_cam.pitch, 0.0);

    commands.spawn((
        EditorCamera,
        fly_cam,
        Camera3d::default(),
        PrimaryEguiContext,
        Hdr,
        Transform::from_translation(Vec3::new(0.0, 5.0, 10.0)).with_rotation(rotation),
        // Enable volumetric fog system (requires FogVolume entities to be visible)
        VolumetricFog {
            ambient_intensity: 0.0,
            ..default()
        },
        // Enable outline rendering for selection indication
        OutlineSettings::default(),
    ));
}

/// Look around with right mouse button drag
fn camera_look(
    mouse_button: Res<ButtonInput<MouseButton>>,
    mouse_motion: Res<AccumulatedMouseMotion>,
    settings: Res<Settings>,
    editor_state: Res<EditorState>,
    _mode: Res<State<EditorMode>>,
    mut query: Query<(&mut FlyCamera, &mut Transform), With<EditorCamera>>,
    mut contexts: EguiContexts,
) {
    if !editor_state.editor_active {
        return;
    }

    // Must hold right mouse button for freelook
    if !mouse_button.pressed(MouseButton::Right) {
        return;
    }

    // Don't capture mouse when UI wants pointer input
    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_pointer_input() || ctx.is_pointer_over_area() {
            return;
        }
    }

    let delta = mouse_motion.delta;
    if delta == Vec2::ZERO {
        return;
    }

    for (mut fly_cam, mut transform) in &mut query {
        // Don't allow rotation in orthographic mode
        if fly_cam.fov_degrees == 0.0 {
            return;
        }

        fly_cam.yaw -= delta.x * settings.camera_sensitivity;
        fly_cam.pitch = (fly_cam.pitch - delta.y * settings.camera_sensitivity)
            .clamp(-std::f32::consts::FRAC_PI_2 + 0.1, std::f32::consts::FRAC_PI_2 - 0.1);

        transform.rotation = Quat::from_euler(EulerRot::YXZ, fly_cam.yaw, fly_cam.pitch, 0.0);
    }
}

/// WASD movement for fly camera
fn camera_movement(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    settings: Res<Settings>,
    mode: Res<State<EditorMode>>,
    editor_state: Res<EditorState>,
    mut query: Query<&mut Transform, With<EditorCamera>>,
    mut contexts: EguiContexts,
) {
    // In Edit mode, only allow camera movement when right mouse button is held
    // (otherwise WASD is used for axis selection)
    if *mode.get() == EditorMode::Edit && !mouse_button.pressed(MouseButton::Right) {
        return;
    }

    if !should_process_input(&editor_state, &mut contexts) {
        return;
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
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    editor_state: Res<EditorState>,
    mut query: Query<(&mut FlyCamera, &mut Projection), With<EditorCamera>>,
    mut contexts: EguiContexts,
) {
    if !editor_state.editor_active {
        return;
    }

    // Don't zoom in Insert mode (scroll used for sub-mode selection)
    if *mode.get() == EditorMode::Insert {
        return;
    }

    // Don't zoom in Edit mode with SnapToObject (scroll used for sub-mode selection)
    if *mode.get() == EditorMode::Edit && *transform_op == TransformOperation::SnapToObject {
        return;
    }

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
        if fly_cam.fov_degrees == 0.0 {
            // In orthographic mode: scroll adjusts scale
            // Scroll up = zoom in (decrease scale), scroll down = zoom out (increase scale)
            let zoom_factor = 1.0 - scroll_y * ORTHO_ZOOM_SPEED;
            let new_scale = fly_cam.ortho_scale * zoom_factor;

            if new_scale >= MAX_ORTHO_SCALE && scroll_y < 0.0 {
                // Zooming out past max scale: switch to perspective
                fly_cam.fov_degrees = MIN_FOV_DEGREES;
                *projection = Projection::Perspective(PerspectiveProjection {
                    fov: fly_cam.fov_degrees.to_radians(),
                    ..default()
                });
            } else {
                // Stay in orthographic, clamp scale
                fly_cam.ortho_scale = new_scale.clamp(MIN_ORTHO_SCALE, MAX_ORTHO_SCALE);
                *projection = Projection::Orthographic(OrthographicProjection {
                    scale: fly_cam.ortho_scale,
                    ..OrthographicProjection::default_3d()
                });
            }
        } else {
            // In perspective mode: scroll adjusts FOV
            // Scroll up = zoom in (decrease FOV), scroll down = zoom out (increase FOV)
            let new_fov = fly_cam.fov_degrees - scroll_y * FOV_SCROLL_SPEED;

            if new_fov <= MIN_FOV_DEGREES && scroll_y > 0.0 {
                // Zooming in past min FOV - switch to orthographic
                fly_cam.fov_degrees = 0.0;
                *projection = Projection::Orthographic(OrthographicProjection {
                    scale: fly_cam.ortho_scale,
                    ..OrthographicProjection::default_3d()
                });
            } else {
                // Stay in perspective mode
                fly_cam.fov_degrees = new_fov.clamp(MIN_FOV_DEGREES, MAX_FOV_DEGREES);
                *projection = Projection::Perspective(PerspectiveProjection {
                    fov: fly_cam.fov_degrees.to_radians(),
                    ..default()
                });
            }
        }
    }
}

/// Handle camera preset switching
fn handle_camera_preset(
    mut events: MessageReader<SetCameraPresetEvent>,
    mut query: Query<(&mut FlyCamera, &mut Transform, &mut Projection), With<EditorCamera>>,
) {
    for event in events.read() {
        let preset = &event.preset;
        let (yaw, pitch) = preset.angles();
        let distance = if event.orthographic { ORTHO_PRESET_DISTANCE } else { PRESET_DISTANCE };
        let position = preset.position(distance);
        for (mut fly_cam, mut transform, mut projection) in &mut query {
            fly_cam.yaw = yaw;
            fly_cam.pitch = pitch;
            transform.translation = position;
            transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);

            // Switch to orthographic if requested
            if event.orthographic {
                fly_cam.fov_degrees = 0.0;
                *projection = Projection::Orthographic(OrthographicProjection {
                    scale: ORTHO_SCALE,
                    ..OrthographicProjection::default_3d()
                });
            }
        }
    }
}

/// Focus/frame the camera on selected objects when L is pressed
fn look_at_selected(
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    selected_query: Query<(&Transform, Option<&Collider>), (With<Selected>, Without<EditorCamera>)>,
    mut camera_query: Query<(&mut FlyCamera, &mut Transform, &Projection), With<EditorCamera>>,
    mut contexts: EguiContexts,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    if !keyboard.just_pressed(KeyCode::KeyL) {
        return;
    }

    // Calculate bounding box of all selected objects
    let mut min_bounds = Vec3::splat(f32::MAX);
    let mut max_bounds = Vec3::splat(f32::MIN);
    let mut count = 0;

    for (transform, collider) in &selected_query {
        count += 1;
        let pos = transform.translation;

        // Get object half-extents from collider or use default
        let half_extents = collider
            .map(|c| {
                let aabb = c.aabb(pos, transform.rotation);
                let min: Vec3 = aabb.min.into();
                let max: Vec3 = aabb.max.into();
                (max - min) * 0.5
            })
            .unwrap_or(Vec3::splat(0.5));

        min_bounds = min_bounds.min(pos - half_extents);
        max_bounds = max_bounds.max(pos + half_extents);
    }

    if count == 0 {
        return;
    }

    // Calculate center and size of bounding box
    let center = (min_bounds + max_bounds) * 0.5;
    let size = max_bounds - min_bounds;
    let max_extent = size.max_element().max(1.0);

    for (mut fly_cam, mut camera_transform, projection) in &mut camera_query {
        // Calculate distance based on FOV and bounding box size
        let distance = match projection {
            Projection::Perspective(persp) => {
                let fov = persp.fov;
                let half_fov = fov * 0.5;
                // Distance needed to fit the object
                (max_extent * FRAME_PADDING) / half_fov.tan()
            }
            Projection::Orthographic(_) | Projection::Custom(_) => {
                // For ortho or custom, use a fixed multiplier
                max_extent * FRAME_PADDING * 2.0
            }
        };

        let distance = distance.max(MIN_FRAME_DISTANCE);

        // 3/4 view offset: diagonal from above
        let offset = Vec3::new(1.0, 0.7, 1.0).normalize() * distance;
        let new_pos = center + offset;

        camera_transform.translation = new_pos;
        camera_transform.look_at(center, Vec3::Y);

        // Extract yaw and pitch from the resulting rotation
        let (yaw, pitch, _) = camera_transform.rotation.to_euler(EulerRot::YXZ);
        fly_cam.yaw = yaw;
        fly_cam.pitch = pitch;
    }
}

/// Sync camera enabled states reactively when editor state or game state changes.
///
/// Game cameras are only active when the editor is inactive AND game is Playing.
/// This replaces both the old event-based sync and the exclusive-world sync in game commands.
fn sync_camera_states(
    editor_state: Res<EditorState>,
    game_state: Res<State<GameState>>,
    mut game_cameras: Query<&mut Camera, (With<GameCamera>, Without<EditorCamera>)>,
) {
    if !editor_state.is_changed() && !game_state.is_changed() {
        return;
    }

    let active = !editor_state.editor_active && *game_state.get() == GameState::Playing;
    for mut camera in &mut game_cameras {
        camera.is_active = active;
    }
}
