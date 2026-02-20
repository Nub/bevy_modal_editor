//! Marble Demo - A marble rolling game using bevy_modal_editor
//!
//! Demonstrates the editor's play/pause/reset lifecycle:
//! 1. Design a level in the editor (ground, ramps, obstacles, spawn point, goal zone)
//! 2. Press F4 to play — marble spawns at SpawnPoint, camera follows
//! 3. WASD to roll the marble, Space to jump
//! 4. Reach the GoalZone to complete the level
//! 5. F6 to pause, F4 to resume, F7 to reset

mod checkerboard;
mod game_camera;
pub mod levels;
mod marble;
mod sounds;
pub mod timer;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy::remote::{RemotePlugin, http::RemoteHttpPlugin};
use bevy_editor_game::{
    AlphaModeValue, BaseMaterialProps, CustomEntityType, MaterialDefinition, MaterialRef,
    RegisterCustomEntityExt, RegisterValidationExt, ValidationMessage, ValidationRule,
    ValidationSeverity,
};
use bevy_modal_editor::materials::RegisterMaterialTypeExt;
use bevy_modal_editor::{EditorPlugin, EditorPluginConfig, GamePlugin, recommended_image_plugin};

use checkerboard::{CheckerboardMaterialDef, CheckerboardMaterialPlugin};

/// Marker component for spawn point entities.
/// The marble spawns at this entity's position when play mode starts.
#[derive(Component, Clone, Default, Reflect, serde::Serialize, serde::Deserialize)]
#[reflect(Component)]
pub struct SpawnPoint;

/// Marker component for goal zone entities.
/// Reaching this zone triggers level completion.
#[derive(Component, Clone, Default, Reflect, serde::Serialize, serde::Deserialize)]
#[reflect(Component)]
pub struct GoalZone;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(recommended_image_plugin()))
        .add_plugins(RemotePlugin::default())
        .add_plugins(RemoteHttpPlugin::default())
        .add_plugins(EditorPlugin::new(EditorPluginConfig {
            pause_physics_on_startup: true,
            ..default()
        }))
        .add_plugins(GamePlugin)
        .add_plugins(CheckerboardMaterialPlugin)
        .register_material_type::<CheckerboardMaterialDef>()
        .add_plugins(marble::MarblePlugin)
        .add_plugins(game_camera::GameCameraPlugin)
        .add_plugins(timer::GameTimerPlugin)
        .add_plugins(levels::LevelsPlugin)
        .add_plugins(sounds::SoundEffectsPlugin)
        .register_custom_entity::<SpawnPoint>(CustomEntityType {
            name: "Spawn Point",
            category: "Game",
            keywords: &["start", "player", "origin", "marble"],
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
        .register_custom_entity::<GoalZone>(CustomEntityType {
            name: "Goal Zone",
            category: "Game",
            keywords: &["finish", "end", "target", "win"],
            default_position: Vec3::new(0.0, 1.0, 0.0),
            spawn: |commands, position, rotation| {
                commands
                    .spawn((
                        GoalZone,
                        Transform::from_translation(position).with_rotation(rotation),
                        Visibility::default(),
                        Collider::cuboid(1.0, 1.0, 1.0),
                        Sensor,
                        MaterialRef::Inline(MaterialDefinition {
                            base: BaseMaterialProps {
                                base_color: Color::srgba(0.2, 1.0, 0.3, 0.3),
                                alpha_mode: AlphaModeValue::Blend,
                                ..default()
                            },
                            extension: None,
                        }),
                    ))
                    .id()
            },
            draw_inspector: None,
            draw_gizmo: Some(draw_goal_zone_gizmo),
            regenerate: Some(regenerate_goal_zone),
        })
        .register_validation(ValidationRule {
            name: "Spawn Point",
            validate: validate_spawn_points,
        })
        .register_validation(ValidationRule {
            name: "Goal Zone",
            validate: validate_goal_zones,
        })
        .run();
}

/// Draw a gizmo for a single spawn point (upward arrow marker).
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

/// Draw a gizmo for a single goal zone (green wireframe sphere).
fn draw_goal_zone_gizmo(gizmos: &mut Gizmos, transform: &GlobalTransform) {
    let pos = transform.translation();
    let color = Color::srgb(0.2, 1.0, 0.3);
    let size = 0.5;
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

/// Regenerate runtime components for a spawn point after scene restore.
fn regenerate_spawn_point(world: &mut World, entity: Entity) {
    if world.get::<Visibility>(entity).is_some() {
        return;
    }
    if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
        entity_mut.insert((Visibility::default(), Collider::sphere(0.3)));
    }
}

/// Regenerate runtime components for a goal zone after scene restore.
fn regenerate_goal_zone(world: &mut World, entity: Entity) {
    if world.get::<Visibility>(entity).is_some() {
        return;
    }
    let mesh_handle = world
        .resource_mut::<Assets<Mesh>>()
        .add(Cuboid::new(1.0, 1.0, 1.0));
    if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
        entity_mut.insert((
            Visibility::default(),
            Collider::cuboid(1.0, 1.0, 1.0),
            Sensor,
            Mesh3d(mesh_handle),
        ));
    }
}

/// Validate that exactly one spawn point exists.
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

/// Validate that at least one goal zone exists.
fn validate_goal_zones(world: &mut World) -> Vec<ValidationMessage> {
    let mut q = world.query_filtered::<Entity, With<GoalZone>>();
    let count = q.iter(world).count();
    if count == 0 {
        vec![ValidationMessage {
            severity: ValidationSeverity::Warning,
            message: "No Goal Zone in scene. Level cannot be completed.".into(),
            entity: None,
        }]
    } else {
        vec![]
    }
}
