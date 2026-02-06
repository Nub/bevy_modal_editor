//! Coin collectible system: spinning, collection, tracking.

use bevy::prelude::*;
use bevy_editor_game::{GameEntity, GameStartedEvent, GameState};
use bevy_locomotion::prelude::*;

use crate::Coin;

/// Tracks coin collection progress.
#[derive(Resource, Default)]
pub struct CoinTracker {
    pub total: usize,
    pub collected: usize,
}

/// Fired when a single coin is collected.
#[derive(Message)]
pub struct CoinCollectedEvent;

/// Fired when all coins are collected.
#[derive(Message)]
pub struct AllCoinsCollectedEvent {
    pub elapsed: f32,
}

/// Runtime marker for active coins during gameplay.
#[derive(Component)]
pub struct CoinRuntime {
    pub base_y: f32,
    pub spin_phase: f32,
}

pub struct CoinPlugin;

impl Plugin for CoinPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CoinTracker>()
            .add_message::<CoinCollectedEvent>()
            .add_message::<AllCoinsCollectedEvent>()
            .add_systems(Update, activate_coins_on_game_start)
            .add_systems(
                Update,
                (spin_coins, check_coin_collection, handle_all_collected)
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

/// Activate coins when game starts: add runtime mesh + sensor.
fn activate_coins_on_game_start(
    mut events: MessageReader<GameStartedEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut tracker: ResMut<CoinTracker>,
    coins: Query<(Entity, &Transform), With<Coin>>,
) {
    for _ in events.read() {
        let count = coins.iter().count();
        *tracker = CoinTracker {
            total: count,
            collected: 0,
        };

        let mesh = meshes.add(Torus::new(0.15, 0.35));
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 0.84, 0.0),
            metallic: 0.9,
            perceptual_roughness: 0.2,
            emissive: Color::srgb(0.5, 0.42, 0.0).into(),
            ..default()
        });

        for (entity, transform) in coins.iter() {
            commands.entity(entity).insert((
                CoinRuntime {
                    base_y: transform.translation.y,
                    spin_phase: 0.0,
                },
                Mesh3d(mesh.clone()),
                MeshMaterial3d(material.clone()),
                GameEntity,
            ));
        }

        info!("Activated {} coins", count);
    }
}

/// Spin and bob coins.
fn spin_coins(time: Res<Time>, mut coins: Query<(&mut Transform, &mut CoinRuntime)>) {
    for (mut transform, mut runtime) in coins.iter_mut() {
        runtime.spin_phase += time.delta_secs();
        // Spin around Y axis
        transform.rotation = Quat::from_rotation_y(runtime.spin_phase * 2.0);
        // Subtle vertical bob
        transform.translation.y =
            runtime.base_y + (runtime.spin_phase * 1.5).sin() * 0.15;
    }
}

/// Check distance between player and coins for collection.
fn check_coin_collection(
    mut commands: Commands,
    mut tracker: ResMut<CoinTracker>,
    mut coin_events: MessageWriter<CoinCollectedEvent>,
    player_query: Query<&Transform, With<Player>>,
    coins: Query<(Entity, &Transform), With<CoinRuntime>>,
) {
    let Ok(player_transform) = player_query.single() else {
        return;
    };
    let player_pos = player_transform.translation;

    for (entity, transform) in coins.iter() {
        let distance = player_pos.distance(transform.translation);
        // Player radius ~0.4 + coin radius ~0.5 = ~0.9, use 1.2 for comfortable pickup
        if distance < 1.2 {
            tracker.collected += 1;
            info!(
                "Coin collected! ({}/{})",
                tracker.collected, tracker.total
            );
            coin_events.write(CoinCollectedEvent);
            commands.entity(entity).despawn();
        }
    }
}

/// Check if all coins have been collected.
fn handle_all_collected(
    tracker: Res<CoinTracker>,
    mut all_events: MessageWriter<AllCoinsCollectedEvent>,
    timer: Res<crate::timer::GameTimer>,
    mut fired: Local<bool>,
) {
    if tracker.total > 0 && tracker.collected >= tracker.total && !*fired {
        *fired = true;
        info!("All coins collected! Time: {:.2}s", timer.elapsed);
        all_events.write(AllCoinsCollectedEvent {
            elapsed: timer.elapsed,
        });
    }

    // Reset fired flag when tracker resets (new game)
    if tracker.collected == 0 {
        *fired = false;
    }
}
