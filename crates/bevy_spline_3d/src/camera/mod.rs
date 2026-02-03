mod fly;
mod orbit;

pub use fly::FlyCamera;
pub use orbit::OrbitCamera;

use bevy::prelude::*;

/// The active camera control mode.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CameraMode {
    /// Orbit camera mode - rotate around a focus point.
    #[default]
    Orbit,
    /// Fly camera mode - FPS-style free movement.
    Fly,
}

impl CameraMode {
    /// Toggle between camera modes.
    pub fn toggle(&mut self) {
        *self = match self {
            Self::Orbit => Self::Fly,
            Self::Fly => Self::Orbit,
        };
    }

    /// Get display name for the current mode.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Orbit => "Orbit",
            Self::Fly => "Fly",
        }
    }
}

/// System to toggle camera mode with F key.
fn toggle_camera_mode(keyboard: Res<ButtonInput<KeyCode>>, mut mode: ResMut<CameraMode>) {
    if keyboard.just_pressed(KeyCode::KeyF) {
        mode.toggle();
    }
}

/// System to sync orbit camera state when switching from fly mode.
fn sync_orbit_from_transform(
    mode: Res<CameraMode>,
    mut cameras: Query<(&mut OrbitCamera, &Transform)>,
) {
    if !mode.is_changed() || *mode != CameraMode::Orbit {
        return;
    }

    let Ok((mut orbit, transform)) = cameras.single_mut() else {
        return;
    };

    // Update orbit parameters from current transform
    let dir = transform.translation - orbit.focus;
    orbit.radius = dir.length();

    if orbit.radius > 0.0 {
        let dir_norm = dir / orbit.radius;
        orbit.pitch = dir_norm.y.asin();
        orbit.yaw = dir_norm.x.atan2(dir_norm.z);
    }
}

/// System to sync fly camera state when switching from orbit mode.
fn sync_fly_from_transform(mode: Res<CameraMode>, mut cameras: Query<(&mut FlyCamera, &Transform)>) {
    if !mode.is_changed() || *mode != CameraMode::Fly {
        return;
    }

    let Ok((mut fly, transform)) = cameras.single_mut() else {
        return;
    };

    // Extract yaw and pitch from current rotation
    let (yaw, pitch, _) = transform.rotation.to_euler(EulerRot::YXZ);
    fly.yaw = yaw;
    fly.pitch = pitch;
}

/// Plugin that adds camera controls with orbit and fly modes.
///
/// # Usage
/// Add both `OrbitCamera` and `FlyCamera` components to your camera entity.
/// The active mode is controlled by the `CameraMode` resource.
///
/// # Controls
/// - `F`: Toggle between orbit and fly modes
///
/// ## Orbit Mode
/// - Right mouse button + drag: Orbit around focus
/// - Scroll wheel: Zoom in/out
///
/// ## Fly Mode
/// - Right mouse button + drag: Look around
/// - WASD: Move forward/back/left/right
/// - Q/Space: Move up
/// - E/Ctrl: Move down
/// - Shift: Sprint
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraMode>().add_systems(
            Update,
            (
                toggle_camera_mode,
                sync_orbit_from_transform,
                sync_fly_from_transform,
                orbit::orbit_camera_input,
                fly::fly_camera_input,
            )
                .chain(),
        );
    }
}
