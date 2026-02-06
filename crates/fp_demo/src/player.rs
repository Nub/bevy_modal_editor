//! Player spawning and locomotion integration.

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::window::{CursorGrabMode, CursorOptions};
use bevy_editor_game::{GameCamera, GameEntity, GameStartedEvent, GameState};
use bevy_locomotion::camera::{CameraPitch, CameraYaw, FpsCamera};
use bevy_locomotion::prelude::*;
use bevy_modal_editor::SceneEntity;

use crate::{LadderSurface, LedgeWall, SpawnPoint, SteepSlope};

pub struct FpPlayerPlugin;

impl Plugin for FpPlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, spawn_player_on_game_start)
            .add_systems(
                Update,
                (tag_locomotion_entities, manage_cursor).run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                unlock_cursor.run_if(
                    state_changed::<GameState>.and(not(in_state(GameState::Playing))),
                ),
            )
            .add_systems(Update, apply_locomotion_markers_on_game_start);
    }
}

/// Spawn the locomotion player when the game starts.
fn spawn_player_on_game_start(
    mut events: MessageReader<GameStartedEvent>,
    mut commands: Commands,
    spawn_points: Query<&Transform, With<SpawnPoint>>,
) {
    for _ in events.read() {
        let spawn_pos = spawn_points
            .iter()
            .next()
            .map(|t| t.translation)
            .unwrap_or(Vec3::new(0.0, 2.0, 0.0));

        info!("Spawning FPS player at {:?}", spawn_pos);

        spawn_player(&mut commands, PlayerConfig::default(), spawn_pos);
    }
}

/// Apply locomotion runtime markers and collision layers to scene entities on game start.
///
/// bevy_locomotion uses custom `GameLayer` collision layers — the player only collides with
/// `GameLayer::World` and `GameLayer::Trigger`. Without explicit layers, scene geometry
/// sits on the default layer and the player falls through everything.
fn apply_locomotion_markers_on_game_start(
    mut events: MessageReader<GameStartedEvent>,
    mut commands: Commands,
    ledge_walls: Query<Entity, With<LedgeWall>>,
    ladder_surfaces: Query<Entity, With<LadderSurface>>,
    steep_slopes: Query<Entity, With<SteepSlope>>,
    scene_colliders: Query<Entity, (With<SceneEntity>, With<Collider>)>,
) {
    let world_layers = CollisionLayers::new(GameLayer::World, [GameLayer::Player]);
    let trigger_layers = CollisionLayers::new(GameLayer::Trigger, [GameLayer::Player]);

    for _ in events.read() {
        // Set all scene colliders to the World layer by default
        for entity in scene_colliders.iter() {
            commands.entity(entity).insert(world_layers);
        }

        // Apply locomotion-specific markers and override layers where needed
        for entity in ledge_walls.iter() {
            commands.entity(entity).insert(LedgeGrabbable);
        }
        for entity in ladder_surfaces.iter() {
            // Ladders must be Sensors on the Trigger layer
            commands
                .entity(entity)
                .insert((Ladder, Sensor, trigger_layers));
        }
        for entity in steep_slopes.iter() {
            commands.entity(entity).insert(ForceSlide);
        }
    }
}

/// Tag locomotion-spawned entities with GameEntity/GameCamera so they're cleaned up on reset.
fn tag_locomotion_entities(
    mut commands: Commands,
    players: Query<Entity, (With<Player>, Without<GameEntity>)>,
    cameras: Query<Entity, (With<FpsCamera>, Without<GameCamera>)>,
    yaws: Query<Entity, (With<CameraYaw>, Without<GameEntity>)>,
    pitches: Query<Entity, (With<CameraPitch>, Without<GameEntity>)>,
) {
    for entity in players.iter() {
        commands.entity(entity).insert(GameEntity);
    }
    for entity in cameras.iter() {
        commands.entity(entity).insert((GameCamera, GameEntity));
    }
    for entity in yaws.iter() {
        commands.entity(entity).insert(GameEntity);
    }
    for entity in pitches.iter() {
        commands.entity(entity).insert(GameEntity);
    }
}

/// Lock and hide cursor during play.
fn manage_cursor(mut cursor_options: Query<&mut CursorOptions>) {
    for mut opts in cursor_options.iter_mut() {
        opts.grab_mode = CursorGrabMode::Locked;
        opts.visible = false;
    }
}

/// Unlock and show cursor when leaving play state.
fn unlock_cursor(mut cursor_options: Query<&mut CursorOptions>) {
    for mut opts in cursor_options.iter_mut() {
        opts.grab_mode = CursorGrabMode::None;
        opts.visible = true;
    }
}
