use bevy::{input::mouse::MouseMotion, prelude::*};

use super::CameraMode;

/// Component for orbit camera behavior.
#[derive(Component, Debug, Clone)]
pub struct OrbitCamera {
    /// The point to orbit around.
    pub focus: Vec3,
    /// Distance from the focus point.
    pub radius: f32,
    /// Rotation around the Y axis (yaw).
    pub yaw: f32,
    /// Rotation around the X axis (pitch).
    pub pitch: f32,
    /// Orbit sensitivity (radians per pixel).
    pub sensitivity: f32,
    /// Zoom sensitivity (units per scroll).
    pub zoom_sensitivity: f32,
    /// Minimum orbit radius.
    pub min_radius: f32,
    /// Maximum orbit radius.
    pub max_radius: f32,
    /// Minimum pitch (prevents flipping).
    pub min_pitch: f32,
    /// Maximum pitch (prevents flipping).
    pub max_pitch: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            radius: 10.0,
            yaw: 0.0,
            pitch: 0.5,
            sensitivity: 0.005,
            zoom_sensitivity: 1.0,
            min_radius: 1.0,
            max_radius: 100.0,
            min_pitch: -std::f32::consts::FRAC_PI_2 + 0.1,
            max_pitch: std::f32::consts::FRAC_PI_2 - 0.1,
        }
    }
}

impl OrbitCamera {
    /// Calculate the camera position from current orbit parameters.
    pub fn calculate_position(&self) -> Vec3 {
        let x = self.radius * self.pitch.cos() * self.yaw.sin();
        let y = self.radius * self.pitch.sin();
        let z = self.radius * self.pitch.cos() * self.yaw.cos();
        self.focus + Vec3::new(x, y, z)
    }
}

/// System to handle orbit camera input.
pub fn orbit_camera_input(
    mut cameras: Query<(&mut OrbitCamera, &mut Transform)>,
    camera_mode: Res<CameraMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut motion: MessageReader<MouseMotion>,
    mut scroll: MessageReader<bevy::input::mouse::MouseWheel>,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if *camera_mode != CameraMode::Orbit {
        motion.clear();
        scroll.clear();
        return;
    }

    let Ok((mut orbit, mut transform)) = cameras.single_mut() else {
        return;
    };

    // Handle orbit rotation (right mouse button or middle mouse button)
    if mouse.pressed(MouseButton::Right) || mouse.pressed(MouseButton::Middle) {
        for ev in motion.read() {
            orbit.yaw -= ev.delta.x * orbit.sensitivity;
            orbit.pitch += ev.delta.y * orbit.sensitivity;
            orbit.pitch = orbit.pitch.clamp(orbit.min_pitch, orbit.max_pitch);
        }
    } else {
        motion.clear();
    }

    // Handle panning (shift + right mouse button)
    if mouse.pressed(MouseButton::Right)
        && (keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight))
    {
        // Pan is handled separately if needed
    }

    // Handle zoom (scroll wheel)
    for ev in scroll.read() {
        orbit.radius -= ev.y * orbit.zoom_sensitivity;
        orbit.radius = orbit.radius.clamp(orbit.min_radius, orbit.max_radius);
    }

    // Update transform
    let position = orbit.calculate_position();
    transform.translation = position;
    transform.look_at(orbit.focus, Vec3::Y);
}
