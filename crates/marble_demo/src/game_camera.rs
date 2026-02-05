use avian3d::prelude::{SpatialQuery, SpatialQueryFilter};
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::render::view::Hdr;
use bevy_editor_game::{GameCamera, GameEntity, GameStartedEvent, GameState};

use crate::marble::Marble;

/// Configuration for the follow camera
#[derive(Resource)]
pub struct FollowCameraConfig {
    /// Distance from the marble
    pub distance: f32,
    /// Mouse sensitivity (radians per pixel)
    pub sensitivity: f32,
    /// Minimum pitch angle (radians, looking down)
    pub min_pitch: f32,
    /// Maximum pitch angle (radians, looking up)
    pub max_pitch: f32,
    /// Offset from the collision surface to avoid z-fighting
    pub collision_offset: f32,
    /// Minimum distance when pushed in by collision
    pub min_distance: f32,
}

impl Default for FollowCameraConfig {
    fn default() -> Self {
        Self {
            distance: 12.0,
            sensitivity: 0.003,
            min_pitch: -1.2,
            max_pitch: 0.2,
            collision_offset: 0.3,
            min_distance: 1.5,
        }
    }
}

/// Orbit state for the camera (yaw/pitch around the marble)
#[derive(Component)]
struct CameraOrbit {
    yaw: f32,
    pitch: f32,
}

impl Default for CameraOrbit {
    fn default() -> Self {
        Self {
            yaw: 0.0,
            pitch: -0.4,
        }
    }
}

pub struct GameCameraPlugin;

impl Plugin for GameCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FollowCameraConfig>()
            .add_systems(Update, spawn_game_camera_on_start)
            .add_systems(
                Update,
                (camera_mouse_input, follow_marble)
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

/// Spawn the game camera when the game starts and activate it.
fn spawn_game_camera_on_start(
    mut events: MessageReader<GameStartedEvent>,
    mut commands: Commands,
) {
    for _ in events.read() {
        commands.spawn((
            GameCamera,
            GameEntity,
            CameraOrbit::default(),
            Camera3d::default(),
            Projection::Perspective(PerspectiveProjection {
                fov: 80.0_f32.to_radians(),
                ..default()
            }),
            Camera {
                is_active: true,
                order: 1,
                ..default()
            },
            Hdr,
            Transform::from_translation(Vec3::new(0.0, 10.0, 15.0))
                .looking_at(Vec3::ZERO, Vec3::Y),
        ));
    }
}

/// Update the camera orbit angles from mouse input
fn camera_mouse_input(
    mouse_motion: Res<AccumulatedMouseMotion>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    config: Res<FollowCameraConfig>,
    mut camera_query: Query<&mut CameraOrbit, With<GameCamera>>,
) {
    // Always rotate — no button required (FPS-style)
    // But skip if right-click is held (reserve for future use)
    if mouse_buttons.pressed(MouseButton::Right) {
        return;
    }

    let delta = mouse_motion.delta;
    if delta == Vec2::ZERO {
        return;
    }

    let Ok(mut orbit) = camera_query.single_mut() else {
        return;
    };

    orbit.yaw -= delta.x * config.sensitivity;
    orbit.pitch = (orbit.pitch - delta.y * config.sensitivity)
        .clamp(config.min_pitch, config.max_pitch);
}

/// Follow the marble with orbit camera + collision avoidance
fn follow_marble(
    config: Res<FollowCameraConfig>,
    marble_query: Query<Entity, With<Marble>>,
    marble_transforms: Query<&Transform, (With<Marble>, Without<GameCamera>)>,
    mut camera_query: Query<
        (&mut Transform, &CameraOrbit),
        (With<GameCamera>, Without<Marble>),
    >,
    spatial_query: SpatialQuery,
) {
    let Ok(marble_entity) = marble_query.single() else {
        return;
    };
    let Ok(marble_transform) = marble_transforms.single() else {
        return;
    };
    let Ok((mut camera_transform, orbit)) = camera_query.single_mut() else {
        return;
    };

    let marble_pos = marble_transform.translation;
    // Focus point slightly above marble center
    let focus = marble_pos + Vec3::Y * 0.5;

    // Compute desired camera offset from orbit angles
    let (yaw_sin, yaw_cos) = orbit.yaw.sin_cos();
    let (pitch_sin, pitch_cos) = orbit.pitch.sin_cos();
    let offset_dir = Vec3::new(
        yaw_sin * pitch_cos,
        -pitch_sin,
        yaw_cos * pitch_cos,
    )
    .normalize();

    // Raycast from focus toward desired camera position to detect obstacles
    let filter = SpatialQueryFilter::default().with_excluded_entities([marble_entity]);
    let actual_distance = if let Some(hit) = spatial_query.cast_ray(
        focus,
        Dir3::new(offset_dir).unwrap_or(Dir3::Y),
        config.distance,
        true,
        &filter,
    ) {
        (hit.distance - config.collision_offset).max(config.min_distance)
    } else {
        config.distance
    };

    let target_pos = focus + offset_dir * actual_distance;

    // Snap to target — no lerp to avoid jitter from physics/render rate mismatch
    camera_transform.translation = target_pos;
    camera_transform.look_at(focus, Vec3::Y);
}
