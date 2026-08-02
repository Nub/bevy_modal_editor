//! Editor viewport navigation (spec §4 / keymap doc): hold RMB to fly — mouse look +
//! WASD planar + Q/E down/up, Shift boosts. Industry-standard, works in every editor
//! mode. Kernel-owned continuous navigation: exempt from the resolver-only rule the
//! way pointer gestures are (chorded ACTIONS stay in the keymap; held-key locomotion
//! is not an action).
//!
//! The editor flies the active camera directly (camera transforms are editor state,
//! not scene content — no undo, no SceneId). The game re-syncs its own view state
//! when it takes input back (template's overlay handles that).

use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions, PrimaryWindow};

use crate::resolver::{EditorState, KeyCapture};

/// True while the RMB fly-nav owns the mouse (selection/placement ignore clicks).
#[derive(Resource, Default)]
pub struct FlyingCamera(pub bool);

/// The camera the editor treats as its viewport: active AND rendering to a window.
///
/// Every editor camera pick MUST go through this predicate (flow-audit rule):
/// "first active camera" breaks the moment any plugin adds an off-screen camera —
/// the outliner's silhouette camera (active, renders to an image) once won that race
/// and silently broke every cursor ray.
pub fn is_viewport_camera(
    camera: &Camera,
    target: Option<&bevy::camera::RenderTarget>,
) -> bool {
    camera.is_active
        && matches!(target, None | Some(bevy::camera::RenderTarget::Window(_)))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn editor_fly_camera(
    state: Res<EditorState>,
    settings: Res<crate::settings::EditorSettings>,
    capture: Res<KeyCapture>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    time: Option<Res<Time>>,
    mut flying: ResMut<FlyingCamera>,
    mut camera: Query<(&Camera, &mut Transform, Option<&bevy::camera::RenderTarget>)>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let was_flying = flying.0;
    let hold = state.active
        && !capture.0
        && mouse.as_ref().is_some_and(|m| m.pressed(MouseButton::Right));
    flying.0 = hold;

    // Cursor policy while the editor owns input: locked during the hold, free
    // otherwise — asserted continuously (idempotent), so entering the editor from the
    // game's locked cursor always frees it (flow-audit: ownership handoff gaps).
    if state.active {
        for mut options in &mut cursor {
            let (grab, visible) =
                if hold { (CursorGrabMode::Locked, false) } else { (CursorGrabMode::None, true) };
            if options.grab_mode != grab {
                options.grab_mode = grab;
                options.visible = visible;
            }
        }
    }
    let _ = was_flying;
    if !hold {
        return;
    }

    let Some((_, mut transform, _)) = camera
        .iter_mut()
        .find(|(camera, _, target)| is_viewport_camera(camera, target.as_deref()))
    else {
        return;
    };

    // Mouse look.
    if let Some(motion) = motion {
        if motion.delta != Vec2::ZERO {
            let (mut yaw, mut pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
            yaw -= motion.delta.x * settings.camera.look_sensitivity;
            pitch = (pitch - motion.delta.y * settings.camera.look_sensitivity).clamp(-1.54, 1.54);
            transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
        }
    }

    // WASD + QE locomotion, camera-relative.
    let Some(keys) = keys else { return };
    let mut wish = Vec3::ZERO;
    let forward = *transform.forward();
    let right = *transform.right();
    if keys.pressed(KeyCode::KeyW) {
        wish += forward;
    }
    if keys.pressed(KeyCode::KeyS) {
        wish -= forward;
    }
    if keys.pressed(KeyCode::KeyD) {
        wish += right;
    }
    if keys.pressed(KeyCode::KeyA) {
        wish -= right;
    }
    if keys.pressed(KeyCode::KeyE) {
        wish += Vec3::Y;
    }
    if keys.pressed(KeyCode::KeyQ) {
        wish -= Vec3::Y;
    }
    let boost = if keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight) {
        settings.camera.fly_boost
    } else {
        1.0
    };
    let Some(time) = time else { return };
    transform.translation +=
        wish.normalize_or_zero() * settings.camera.fly_speed * boost * time.delta_secs();
}
