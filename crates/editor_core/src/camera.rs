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
pub fn is_viewport_camera(camera: &Camera, target: Option<&bevy::camera::RenderTarget>) -> bool {
    camera.is_active && matches!(target, None | Some(bevy::camera::RenderTarget::Window(_)))
}

/// Wheel = zoom, the way it works in every 3D tool a designer has ever used.
///
/// Nothing handled the wheel at all: scrolling in the viewport did nothing, and
/// getting closer to a piece meant holding the right button and flying there.
///
/// A perspective view DOLLIES — moves along its own forward axis — rather than
/// changing the field of view, because narrowing the fov to "zoom" warps the
/// perspective and makes a wall you are trying to mate look like a different
/// shape. An orthographic view has no distance to give, so its scale changes
/// instead, which is the same gesture meaning the same thing in a projection
/// that cannot dolly.
///
/// Kernel-owned continuous navigation, exempt from the resolver-only rule for
/// the same reason fly-nav and pointer gestures are: this is locomotion, not a
/// bindable action. Shift boosts it, matching the fly camera.
pub(crate) fn editor_zoom_camera(
    state: Res<EditorState>,
    settings: Res<crate::settings::EditorSettings>,
    capture: Res<KeyCapture>,
    over_chrome: Res<crate::resolver::PointerOverChrome>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut camera: Query<(
        &Camera,
        &mut Transform,
        &mut Projection,
        Option<&bevy::camera::RenderTarget>,
    )>,
) {
    // Over a panel the wheel belongs to that panel's scrollbar, and while a
    // text field has the keyboard the viewport is not what is being driven.
    if !state.active || capture.0 || over_chrome.0 {
        wheel.clear();
        return;
    }
    let notches: f32 = wheel
        .read()
        .map(|event| match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => event.y,
            // Trackpads report pixels — a swipe is many small events, so scale
            // them into the same units a mouse notch speaks.
            bevy::input::mouse::MouseScrollUnit::Pixel => event.y / 50.0,
        })
        .sum();
    if notches == 0.0 {
        return;
    }
    let boosted = keys
        .map(|keys| keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight))
        .unwrap_or(false);
    let step = settings.camera.zoom_step
        * if boosted {
            settings.camera.fly_boost
        } else {
            1.0
        };
    for (camera, mut transform, mut projection, target) in &mut camera {
        if !is_viewport_camera(camera, target) {
            continue;
        }
        match &mut *projection {
            Projection::Orthographic(ortho) => {
                // Multiplicative, so every notch changes the view by the same
                // PROPORTION — additive zoom crawls when far out and slams into
                // zero when close in.
                ortho.scale = (ortho.scale * (-notches * 0.1).exp()).clamp(0.01, 1000.0);
            }
            _ => {
                let forward = transform.forward();
                transform.translation += forward * notches * step;
            }
        }
    }
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
    mut camera: Query<(
        &Camera,
        &mut Transform,
        &mut Projection,
        Option<&bevy::camera::RenderTarget>,
    )>,
    mut cursor: Query<&mut CursorOptions, With<PrimaryWindow>>,
) {
    let was_flying = flying.0;
    let hold = state.active
        && !capture.0
        && mouse
            .as_ref()
            .is_some_and(|m| m.pressed(MouseButton::Right));
    flying.0 = hold;

    // Cursor policy while the editor owns input: locked during the hold, free
    // otherwise — asserted continuously (idempotent), so entering the editor from the
    // game's locked cursor always frees it (flow-audit: ownership handoff gaps).
    if state.active {
        for mut options in &mut cursor {
            let (grab, visible) = if hold {
                (CursorGrabMode::Locked, false)
            } else {
                (CursorGrabMode::None, true)
            };
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

    let Some((_, mut transform, mut projection, _)) = camera
        .iter_mut()
        .find(|(camera, _, _, target)| is_viewport_camera(camera, target.as_deref()))
    else {
        return;
    };
    // Flying LEAVES a canonical view: hand perspective back, or navigation
    // happens inside an orthographic box and reads as nothing moving.
    if matches!(*projection, Projection::Orthographic(_)) {
        *projection = Projection::Perspective(PerspectiveProjection::default());
    }

    // Mouse look.
    if let Some(motion) = motion
        && motion.delta != Vec2::ZERO
    {
        let (mut yaw, mut pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
        yaw -= motion.delta.x * settings.camera.look_sensitivity;
        pitch = (pitch - motion.delta.y * settings.camera.look_sensitivity).clamp(-1.54, 1.54);
        transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, 0.0);
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

use crate::selection::Selected;
use editor_api::prelude::{ActionInvoked, SceneId};

/// `zz` / `zf` (spec §4, keymap doc): frame the selection (or the whole scene)
/// — keep the current view direction, pull back far enough that everything
/// selected fits with padding. The keyboard-first answer to "where did it go".
pub(crate) fn handle_frame_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<EditorState>,
    selected: Query<&GlobalTransform, (With<Selected>, With<SceneId>)>,
    everything: Query<&GlobalTransform, With<SceneId>>,
    mut cameras: Query<(&Camera, &mut Transform, Option<&bevy::camera::RenderTarget>)>,
) {
    for invoked in reader.read() {
        let frame_selection = invoked.action.as_str() == "camera.frame";
        let frame_scene = invoked.action.as_str() == "camera.frame-scene";
        if !(frame_selection || frame_scene) || !state.active {
            continue;
        }
        let points: Vec<Vec3> = if frame_selection && !selected.is_empty() {
            selected.iter().map(|t| t.translation()).collect()
        } else {
            everything.iter().map(|t| t.translation()).collect()
        };
        if points.is_empty() {
            continue;
        }
        let center = points.iter().sum::<Vec3>() / points.len() as f32;
        let radius = points
            .iter()
            .map(|p| p.distance(center))
            .fold(1.0f32, f32::max)
            + 1.5;
        let Some((_, mut transform, _)) = cameras
            .iter_mut()
            .find(|(camera, _, target)| is_viewport_camera(camera, target.as_deref()))
        else {
            continue;
        };
        let back = transform
            .forward()
            .try_normalize()
            .unwrap_or(bevy::math::Dir3::NEG_Z.as_vec3());
        let distance = radius * 2.4;
        transform.translation = center - back * distance + Vec3::Y * (radius * 0.35);
        transform.look_at(center, Vec3::Y);
    }
}

/// MMB (or Alt+LMB) orbit around the selection — the missing third leg of
/// navigation (fly = RMB, frame = zz). Pivot = selection centroid at drag
/// start, or the point ~8m ahead when nothing is selected.
pub(crate) fn orbit_camera(
    state: Res<crate::resolver::EditorState>,
    over_chrome: Res<crate::resolver::PointerOverChrome>,
    mouse: Option<Res<ButtonInput<MouseButton>>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    motion: Option<Res<AccumulatedMouseMotion>>,
    selected: Query<&GlobalTransform, (With<Selected>, With<SceneId>)>,
    mut pivot: Local<Option<Vec3>>,
    mut cameras: Query<(&Camera, &mut Transform, Option<&bevy::camera::RenderTarget>)>,
) {
    let (Some(mouse), Some(keys), Some(motion)) = (mouse, keys, motion) else {
        return;
    };
    if !state.active {
        *pivot = None;
        return;
    }
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    let held = mouse.pressed(MouseButton::Middle) || (alt && mouse.pressed(MouseButton::Left));
    let started =
        mouse.just_pressed(MouseButton::Middle) || (alt && mouse.just_pressed(MouseButton::Left));
    if !held {
        *pivot = None;
        return;
    }
    let Some((_, mut transform, _)) = cameras
        .iter_mut()
        .find(|(camera, _, target)| is_viewport_camera(camera, target.as_deref()))
    else {
        return;
    };
    if started && !over_chrome.0 {
        let points: Vec<Vec3> = selected.iter().map(|t| t.translation()).collect();
        *pivot = Some(if points.is_empty() {
            transform.translation + *transform.forward() * 8.0
        } else {
            points.iter().sum::<Vec3>() / points.len() as f32
        });
    }
    let Some(center) = *pivot else {
        return;
    };
    let delta = motion.delta;
    if delta == Vec2::ZERO {
        return;
    }
    let yaw = Quat::from_rotation_y(-delta.x * 0.005);
    let pitch = Quat::from_axis_angle(*transform.right(), -delta.y * 0.005);
    let rotated = yaw * pitch * (transform.translation - center);
    // Refuse the pitch component near the poles (no gimbal flip).
    let candidate = if rotated.normalize_or_zero().dot(Vec3::Y).abs() > 0.98 {
        yaw * (transform.translation - center)
    } else {
        rotated
    };
    transform.translation = center + candidate;
    transform.look_at(center, Vec3::Y);
}

/// The six canonical views (keymap: `1` front, `2` left, `3` top; hold shift
/// for the opposite face). Axis-aligned and ORTHOGRAPHIC, because that is what
/// a "front view" is for: reading alignment and extents without perspective
/// lying about them.
///
/// Flying restores perspective — leaving the canonical view is exactly the
/// moment you want depth back.
fn axis_view(action: &str) -> Option<(Vec3, Vec3)> {
    // (direction the camera looks ALONG, up)
    let view = match action {
        "view.front" => (Vec3::NEG_Z, Vec3::Y),
        "view.back" => (Vec3::Z, Vec3::Y),
        "view.left" => (Vec3::X, Vec3::Y),
        "view.right" => (Vec3::NEG_X, Vec3::Y),
        // Looking straight down, "up" on screen is world -Z (north).
        "view.top" => (Vec3::NEG_Y, Vec3::NEG_Z),
        "view.bottom" => (Vec3::Y, Vec3::Z),
        _ => return None,
    };
    Some(view)
}

/// `4`: back to the normal perspective view, keeping where you are looking.
pub(crate) fn handle_perspective_view(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<EditorState>,
    mut cameras: Query<(
        &Camera,
        &mut Projection,
        Option<&bevy::camera::RenderTarget>,
    )>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() != "view.perspective" || !state.active {
            continue;
        }
        let Some((_, mut projection, _)) = cameras
            .iter_mut()
            .find(|(camera, _, target)| is_viewport_camera(camera, target.as_deref()))
        else {
            continue;
        };
        if matches!(*projection, Projection::Orthographic(_)) {
            *projection = Projection::Perspective(PerspectiveProjection::default());
        }
    }
}

pub(crate) fn handle_axis_views(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<EditorState>,
    selected: Query<&GlobalTransform, (With<Selected>, With<SceneId>)>,
    everything: Query<&GlobalTransform, With<SceneId>>,
    mut cameras: Query<(
        &Camera,
        &mut Transform,
        &mut Projection,
        Option<&bevy::camera::RenderTarget>,
    )>,
) {
    for invoked in reader.read() {
        let Some((along, up)) = axis_view(invoked.action.as_str()) else {
            continue;
        };
        if !state.active {
            continue;
        }
        // Frame what you are looking at: the selection, else the whole scene.
        let points: Vec<Vec3> = if selected.is_empty() {
            everything.iter().map(|t| t.translation()).collect()
        } else {
            selected.iter().map(|t| t.translation()).collect()
        };
        let (center, radius) = if points.is_empty() {
            (Vec3::ZERO, 8.0)
        } else {
            let center = points.iter().sum::<Vec3>() / points.len() as f32;
            let radius = points
                .iter()
                .map(|p| p.distance(center))
                .fold(1.0f32, f32::max)
                + 1.5;
            (center, radius)
        };
        let Some((_, mut transform, mut projection, _)) = cameras
            .iter_mut()
            .find(|(camera, _, _, target)| is_viewport_camera(camera, target.as_deref()))
        else {
            continue;
        };
        // Stand off far enough that near-plane clipping can't eat the subject;
        // with an orthographic projection the distance costs nothing visually.
        transform.translation = center - along * (radius * 4.0);
        transform.look_to(along, up);
        *projection = Projection::Orthographic(OrthographicProjection {
            scaling_mode: bevy::camera::ScalingMode::AutoMin {
                min_width: radius * 2.4,
                min_height: radius * 2.4,
            },
            ..OrthographicProjection::default_3d()
        });
    }
}

#[cfg(test)]
mod zoom_tests {
    use super::*;
    use bevy::input::mouse::{MouseScrollUnit, MouseWheel};

    fn zoom_app() -> App {
        let mut app = App::new();
        app.init_resource::<EditorState>()
            .init_resource::<KeyCapture>()
            .init_resource::<crate::resolver::PointerOverChrome>()
            .init_resource::<crate::settings::EditorSettings>()
            .add_message::<MouseWheel>()
            .add_systems(Update, editor_zoom_camera);
        app.world_mut().resource_mut::<EditorState>().active = true;
        app
    }

    fn viewport_camera(app: &mut App) -> Entity {
        app.world_mut()
            .spawn((
                Camera::default(),
                Transform::from_xyz(0.0, 0.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
                Projection::Perspective(PerspectiveProjection::default()),
            ))
            .id()
    }

    fn scroll(app: &mut App, y: f32, unit: MouseScrollUnit) {
        app.world_mut().write_message(MouseWheel {
            unit,
            x: 0.0,
            y,
            window: Entity::PLACEHOLDER,
            phase: bevy::input::touch::TouchPhase::Moved,
        });
    }

    /// A perspective view DOLLIES: scrolling up moves the camera along its own
    /// forward axis, toward what it is looking at.
    #[test]
    fn the_wheel_dollies_a_perspective_view() {
        let mut app = zoom_app();
        let camera = viewport_camera(&mut app);
        scroll(&mut app, 1.0, MouseScrollUnit::Line);
        app.update();
        let after = app.world().get::<Transform>(camera).unwrap().translation;
        assert!(after.z < 10.0, "scrolling up moved it closer: {after:?}");
        assert!(
            after.x.abs() < 1e-5 && after.y.abs() < 1e-5,
            "along forward only"
        );

        let before = after;
        scroll(&mut app, -1.0, MouseScrollUnit::Line);
        app.update();
        let back = app.world().get::<Transform>(camera).unwrap().translation;
        assert!(back.z > before.z, "and scrolling down pulled it out");
    }

    /// The fov must NOT be what changes: narrowing it warps the perspective, so
    /// a wall you are lining up looks like a different shape as you approach.
    #[test]
    fn zooming_never_touches_the_field_of_view() {
        let mut app = zoom_app();
        let camera = viewport_camera(&mut app);
        let fov = match app.world().get::<Projection>(camera).unwrap() {
            Projection::Perspective(p) => p.fov,
            _ => unreachable!(),
        };
        scroll(&mut app, 3.0, MouseScrollUnit::Line);
        app.update();
        match app.world().get::<Projection>(camera).unwrap() {
            Projection::Perspective(p) => assert_eq!(p.fov, fov),
            _ => panic!("still perspective"),
        }
    }

    /// An orthographic view has no distance to give, so the same gesture scales
    /// it — multiplicatively, so each notch is the same proportion.
    #[test]
    fn an_orthographic_view_scales_instead() {
        let mut app = zoom_app();
        let camera = app
            .world_mut()
            .spawn((
                Camera::default(),
                Transform::from_xyz(0.0, 50.0, 0.0).looking_at(Vec3::ZERO, Vec3::Z),
                Projection::Orthographic(OrthographicProjection::default_3d()),
            ))
            .id();
        let before = match app.world().get::<Projection>(camera).unwrap() {
            Projection::Orthographic(o) => o.scale,
            _ => unreachable!(),
        };
        scroll(&mut app, 2.0, MouseScrollUnit::Line);
        app.update();
        let after = match app.world().get::<Projection>(camera).unwrap() {
            Projection::Orthographic(o) => o.scale,
            _ => unreachable!(),
        };
        assert!(after < before, "{before} -> {after}");
        let moved = app.world().get::<Transform>(camera).unwrap().translation;
        assert_eq!(moved, Vec3::new(0.0, 50.0, 0.0), "and does not dolly");
    }

    /// Over a panel the wheel belongs to that panel's scrollbar. Stealing it
    /// for the camera is how a list becomes impossible to scroll.
    #[test]
    fn the_wheel_over_chrome_is_not_the_cameras() {
        let mut app = zoom_app();
        let camera = viewport_camera(&mut app);
        app.world_mut()
            .resource_mut::<crate::resolver::PointerOverChrome>()
            .0 = true;
        scroll(&mut app, 1.0, MouseScrollUnit::Line);
        app.update();
        let after = app.world().get::<Transform>(camera).unwrap().translation;
        assert_eq!(after.z, 10.0, "the viewport did not move");
    }

    /// A trackpad reports pixels, a mouse reports lines. One swipe must not
    /// teleport the camera a hundred metres.
    #[test]
    fn a_trackpad_swipe_is_not_a_hundred_notches() {
        let mut app = zoom_app();
        let camera = viewport_camera(&mut app);
        scroll(&mut app, 50.0, MouseScrollUnit::Pixel);
        app.update();
        let after = app.world().get::<Transform>(camera).unwrap().translation;
        let travelled = 10.0 - after.z;
        assert!(
            (travelled - 0.9).abs() < 1e-4,
            "50 pixels is one notch, not fifty: travelled {travelled}"
        );
    }
}
