use bevy::prelude::*;
use bevy_modal_editor::{GameCamera, SimulationState};

use crate::marble::Marble;

/// Configuration for the follow camera
#[derive(Resource)]
pub struct FollowCameraConfig {
    /// Distance behind the marble
    pub distance: f32,
    /// Height above the marble
    pub height: f32,
    /// How fast the camera follows (lerp speed)
    pub smoothing: f32,
}

impl Default for FollowCameraConfig {
    fn default() -> Self {
        Self {
            distance: 10.0,
            height: 6.0,
            smoothing: 3.0,
        }
    }
}

pub struct GameCameraPlugin;

impl Plugin for GameCameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FollowCameraConfig>()
            .add_systems(OnEnter(SimulationState::Playing), spawn_game_camera)
            .add_systems(OnEnter(SimulationState::Editing), despawn_game_camera)
            .add_systems(
                Update,
                follow_marble.run_if(in_state(SimulationState::Playing)),
            );
    }
}

/// Spawn the game camera when entering play mode and activate it.
/// The editor camera stays active (for egui rendering) — this camera
/// renders at a higher order so its output is what the player sees.
fn spawn_game_camera(
    mut commands: Commands,
    existing: Query<Entity, With<GameCamera>>,
) {
    // Don't spawn if already exists (resuming from pause)
    if !existing.is_empty() {
        return;
    }

    commands.spawn((
        GameCamera,
        Camera3d::default(),
        Camera {
            is_active: true,
            order: 1,
            ..default()
        },
        Transform::from_translation(Vec3::new(0.0, 10.0, 15.0))
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));
}

/// Despawn the game camera when resetting to editing
fn despawn_game_camera(
    mut commands: Commands,
    cameras: Query<Entity, With<GameCamera>>,
) {
    for entity in cameras.iter() {
        commands.entity(entity).despawn();
    }
}

/// Follow the marble with a smooth third-person camera
fn follow_marble(
    time: Res<Time>,
    config: Res<FollowCameraConfig>,
    marble_query: Query<&Transform, (With<Marble>, Without<GameCamera>)>,
    mut camera_query: Query<&mut Transform, (With<GameCamera>, Without<Marble>)>,
) {
    let Ok(marble_transform) = marble_query.single() else {
        return;
    };
    let Ok(mut camera_transform) = camera_query.single_mut() else {
        return;
    };

    let marble_pos = marble_transform.translation;

    // Target camera position: behind and above the marble
    // Use a fixed offset direction (looking from behind along +Z)
    let target_pos = marble_pos + Vec3::new(0.0, config.height, config.distance);

    // Smoothly interpolate camera position
    let dt = time.delta_secs();
    camera_transform.translation = camera_transform
        .translation
        .lerp(target_pos, (config.smoothing * dt).min(1.0));

    // Always look at the marble
    camera_transform.look_at(marble_pos, Vec3::Y);
}
