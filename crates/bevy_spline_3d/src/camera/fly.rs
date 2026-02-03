use bevy::{input::mouse::MouseMotion, prelude::*};

use super::CameraMode;

/// Component for fly camera behavior.
#[derive(Component, Debug, Clone)]
pub struct FlyCamera {
    /// Movement speed in units per second.
    pub speed: f32,
    /// Sprint speed multiplier.
    pub sprint_multiplier: f32,
    /// Mouse look sensitivity (radians per pixel).
    pub sensitivity: f32,
    /// Current yaw rotation.
    pub yaw: f32,
    /// Current pitch rotation.
    pub pitch: f32,
    /// Minimum pitch (prevents flipping).
    pub min_pitch: f32,
    /// Maximum pitch (prevents flipping).
    pub max_pitch: f32,
}

impl Default for FlyCamera {
    fn default() -> Self {
        Self {
            speed: 5.0,
            sprint_multiplier: 2.5,
            sensitivity: 0.003,
            yaw: 0.0,
            pitch: 0.0,
            min_pitch: -std::f32::consts::FRAC_PI_2 + 0.1,
            max_pitch: std::f32::consts::FRAC_PI_2 - 0.1,
        }
    }
}

/// System to handle fly camera input.
pub fn fly_camera_input(
    mut cameras: Query<(&mut FlyCamera, &mut Transform)>,
    camera_mode: Res<CameraMode>,
    mouse: Res<ButtonInput<MouseButton>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut motion: MessageReader<MouseMotion>,
    time: Res<Time>,
) {
    if *camera_mode != CameraMode::Fly {
        motion.clear();
        return;
    }

    let Ok((mut fly, mut transform)) = cameras.single_mut() else {
        return;
    };

    // Handle mouse look (right mouse button held)
    if mouse.pressed(MouseButton::Right) {
        for ev in motion.read() {
            fly.yaw -= ev.delta.x * fly.sensitivity;
            fly.pitch -= ev.delta.y * fly.sensitivity;
            fly.pitch = fly.pitch.clamp(fly.min_pitch, fly.max_pitch);
        }
    } else {
        motion.clear();
    }

    // Calculate movement direction
    let mut direction = Vec3::ZERO;

    if keyboard.pressed(KeyCode::KeyW) {
        direction += *transform.forward();
    }
    if keyboard.pressed(KeyCode::KeyS) {
        direction -= *transform.forward();
    }
    if keyboard.pressed(KeyCode::KeyA) {
        direction -= *transform.right();
    }
    if keyboard.pressed(KeyCode::KeyD) {
        direction += *transform.right();
    }
    if keyboard.pressed(KeyCode::KeyQ) || keyboard.pressed(KeyCode::Space) {
        direction += Vec3::Y;
    }
    if keyboard.pressed(KeyCode::KeyE) || keyboard.pressed(KeyCode::ControlLeft) {
        direction -= Vec3::Y;
    }

    // Apply movement
    if direction != Vec3::ZERO {
        let speed = if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight)
        {
            fly.speed * fly.sprint_multiplier
        } else {
            fly.speed
        };

        transform.translation += direction.normalize() * speed * time.delta_secs();
    }

    // Apply rotation
    transform.rotation = Quat::from_euler(EulerRot::YXZ, fly.yaw, fly.pitch, 0.0);
}
