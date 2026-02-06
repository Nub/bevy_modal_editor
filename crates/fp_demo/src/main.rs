//! First-Person Demo — A first-person platformer using bevy_locomotion
//!
//! Demonstrates locomotion integration with bevy_modal_editor:
//! 1. Design a level in the editor with locomotion markers
//! 2. Press F5 to play — FPS player spawns at SpawnPoint
//! 3. Navigate the arena, collect all 10 coins
//! 4. Timer tracks from first movement to all coins collected
//! 5. F7 to reset, F5 to replay

mod coins;
pub mod levels;
mod player;
mod sounds;
pub mod timer;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_editor_game::{
    CustomEntityType, RegisterCustomEntityExt, RegisterSceneComponentExt, RegisterValidationExt,
    ValidationMessage, ValidationRule, ValidationSeverity,
};
use bevy_locomotion::prelude::*;
use bevy_modal_editor::{EditorPlugin, EditorPluginConfig, GamePlugin, recommended_image_plugin};

/// Marker component for player spawn point.
#[derive(Component, Clone, Default, Reflect, serde::Serialize, serde::Deserialize)]
#[reflect(Component)]
pub struct SpawnPoint;

/// Marker component for collectible coins.
#[derive(Component, Clone, Default, Reflect, serde::Serialize, serde::Deserialize)]
#[reflect(Component)]
pub struct Coin;

/// Scene-serializable marker for ledge-grabbable walls.
/// Maps to `LedgeGrabbable` at runtime.
#[derive(Component, Clone, Default, Reflect, serde::Serialize, serde::Deserialize)]
#[reflect(Component)]
pub struct LedgeWall;

/// Scene-serializable marker for ladder surfaces.
/// Maps to `Ladder` + `Sensor` at runtime.
#[derive(Component, Clone, Default, Reflect, serde::Serialize, serde::Deserialize)]
#[reflect(Component)]
pub struct LadderSurface;

/// Scene-serializable marker for steep slopes that force sliding.
/// Maps to `ForceSlide` at runtime.
#[derive(Component, Clone, Default, Reflect, serde::Serialize, serde::Deserialize)]
#[reflect(Component)]
pub struct SteepSlope;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(recommended_image_plugin()))
        .add_plugins(EditorPlugin::new(EditorPluginConfig {
            pause_physics_on_startup: true,
            ..default()
        }))
        .add_plugins(GamePlugin)
        // Add locomotion sub-plugins WITHOUT PhysicsPlugin (editor already adds Avian3D)
        .add_plugins(PlayerPlugin)
        .add_plugins(CameraPlugin)
        // Game systems
        .add_plugins(player::FpPlayerPlugin)
        .add_plugins(coins::CoinPlugin)
        .add_plugins(timer::GameTimerPlugin)
        .add_plugins(levels::LevelsPlugin)
        .add_plugins(sounds::SoundEffectsPlugin)
        // Register scene-serializable locomotion markers
        .register_scene_component::<LedgeWall>()
        .register_scene_component::<LadderSurface>()
        .register_scene_component::<SteepSlope>()
        // Custom entity registrations
        .register_custom_entity::<SpawnPoint>(CustomEntityType {
            name: "Spawn Point",
            category: "Game",
            keywords: &["start", "player", "origin", "spawn"],
            default_position: Vec3::new(0.0, 1.0, 0.0),
            spawn: |commands, position, rotation| {
                commands
                    .spawn((
                        SpawnPoint,
                        Transform::from_translation(position).with_rotation(rotation),
                        Visibility::default(),
                        Collider::sphere(0.3),
                    ))
                    .id()
            },
            draw_inspector: None,
            draw_gizmo: Some(draw_spawn_point_gizmo),
            regenerate: Some(regenerate_spawn_point),
        })
        .register_custom_entity::<Coin>(CustomEntityType {
            name: "Coin",
            category: "Game",
            keywords: &["collectible", "pickup", "token", "gold"],
            default_position: Vec3::new(0.0, 1.5, 0.0),
            spawn: |commands, position, rotation| {
                commands
                    .spawn((
                        Coin,
                        Transform::from_translation(position).with_rotation(rotation),
                        Visibility::default(),
                        Collider::sphere(0.5),
                        Sensor,
                    ))
                    .id()
            },
            draw_inspector: None,
            draw_gizmo: Some(draw_coin_gizmo),
            regenerate: Some(regenerate_coin),
        })
        // Validation rules
        .register_validation(ValidationRule {
            name: "Spawn Point",
            validate: validate_spawn_points,
        })
        .register_validation(ValidationRule {
            name: "Coins",
            validate: validate_coins,
        })
        .run();
}

// ---------------------------------------------------------------------------
// Gizmos
// ---------------------------------------------------------------------------

fn draw_spawn_point_gizmo(gizmos: &mut Gizmos, transform: &GlobalTransform) {
    let pos = transform.translation();
    let color = Color::srgb(0.2, 0.8, 1.0);
    let size = 0.4;
    gizmos.circle(Isometry3d::new(pos, Quat::IDENTITY), size, color);
    gizmos.circle(
        Isometry3d::new(pos, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        size,
        color,
    );
    // Upward arrow
    gizmos.line(pos, pos + Vec3::Y * 1.0, color);
    gizmos.line(
        pos + Vec3::Y * 1.0,
        pos + Vec3::new(0.15, 0.7, 0.0),
        color,
    );
    gizmos.line(
        pos + Vec3::Y * 1.0,
        pos + Vec3::new(-0.15, 0.7, 0.0),
        color,
    );
}

fn draw_coin_gizmo(gizmos: &mut Gizmos, transform: &GlobalTransform) {
    let pos = transform.translation();
    let color = Color::srgb(1.0, 0.84, 0.0); // Gold
    let size = 0.4;
    // Three rings like a spinning coin
    gizmos.circle(Isometry3d::new(pos, Quat::IDENTITY), size, color);
    gizmos.circle(
        Isometry3d::new(pos, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        size,
        color,
    );
    gizmos.circle(
        Isometry3d::new(pos, Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
        size,
        color,
    );
}

// ---------------------------------------------------------------------------
// Regenerate
// ---------------------------------------------------------------------------

fn regenerate_spawn_point(world: &mut World, entity: Entity) {
    if world.get::<Visibility>(entity).is_some() {
        return;
    }
    if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
        entity_mut.insert((Visibility::default(), Collider::sphere(0.3)));
    }
}

fn regenerate_coin(world: &mut World, entity: Entity) {
    if world.get::<Visibility>(entity).is_some() {
        return;
    }
    if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
        entity_mut.insert((Visibility::default(), Collider::sphere(0.5), Sensor));
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

fn validate_spawn_points(world: &mut World) -> Vec<ValidationMessage> {
    let mut q = world.query_filtered::<Entity, With<SpawnPoint>>();
    let count = q.iter(world).count();
    if count == 0 {
        vec![ValidationMessage {
            severity: ValidationSeverity::Error,
            message: "No Spawn Point in scene. Add one for play mode.".into(),
            entity: None,
        }]
    } else if count > 1 {
        let entities: Vec<Entity> = q.iter(world).collect();
        entities
            .into_iter()
            .skip(1)
            .map(|e| ValidationMessage {
                severity: ValidationSeverity::Warning,
                message: "Extra Spawn Point — only the first is used.".into(),
                entity: Some(e),
            })
            .collect()
    } else {
        vec![]
    }
}

fn validate_coins(world: &mut World) -> Vec<ValidationMessage> {
    let mut q = world.query_filtered::<Entity, With<Coin>>();
    let count = q.iter(world).count();
    if count == 0 {
        vec![ValidationMessage {
            severity: ValidationSeverity::Warning,
            message: "No Coins in scene. Add some for gameplay.".into(),
            entity: None,
        }]
    } else {
        vec![]
    }
}
