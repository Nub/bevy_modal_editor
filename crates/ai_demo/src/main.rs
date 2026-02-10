//! AI Demo - Demonstrates navmesh generation and agent pathfinding
//!
//! Uses bevy_modal_editor's built-in navigation (rerecast + landmass):
//! 1. Design a level with obstacles in the editor
//! 2. Press ; to enter AI mode → Generate Navmesh
//! 3. Place SpawnPoint and Waypoint entities via Insert mode
//! 4. Press F4 to play — agents spawn at SpawnPoints and navigate toward Waypoints
//! 5. F7 to reset — agents despawn, navmesh persists

mod agent;
mod levels;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_editor_game::{
    CustomEntityType, RegisterCustomEntityExt, RegisterValidationExt, ValidationMessage,
    ValidationRule, ValidationSeverity,
};
use bevy_modal_editor::{EditorPlugin, EditorPluginConfig, GamePlugin, recommended_image_plugin};

/// Marker for spawn point entities. Agents spawn here when play starts.
#[derive(Component, Clone, Default, Reflect, serde::Serialize, serde::Deserialize)]
#[reflect(Component)]
pub struct SpawnPoint;

/// Marker for waypoint entities. Agents navigate toward the nearest one.
#[derive(Component, Clone, Default, Reflect, serde::Serialize, serde::Deserialize)]
#[reflect(Component)]
pub struct Waypoint;

/// Marker for AI agent placeholders in the editor (not spawned at runtime).
#[derive(Component, Clone, Default, Reflect, serde::Serialize, serde::Deserialize)]
#[reflect(Component)]
pub struct AiAgent;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(recommended_image_plugin()))
        .add_plugins(EditorPlugin::new(EditorPluginConfig {
            pause_physics_on_startup: true,
            ..default()
        }))
        .add_plugins(GamePlugin)
        .add_plugins(levels::LevelsPlugin)
        .add_plugins(agent::AgentPlugin)
        .register_custom_entity::<SpawnPoint>(CustomEntityType {
            name: "Spawn Point",
            category: "AI",
            keywords: &["start", "agent", "spawn"],
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
        .register_custom_entity::<Waypoint>(CustomEntityType {
            name: "Waypoint",
            category: "AI",
            keywords: &["target", "goal", "navigate", "destination"],
            default_position: Vec3::new(5.0, 0.5, 0.0),
            spawn: |commands, position, rotation| {
                commands
                    .spawn((
                        Waypoint,
                        Transform::from_translation(position).with_rotation(rotation),
                        Visibility::default(),
                        Collider::sphere(0.3),
                    ))
                    .id()
            },
            draw_inspector: None,
            draw_gizmo: Some(draw_waypoint_gizmo),
            regenerate: Some(regenerate_waypoint),
        })
        .register_validation(ValidationRule {
            name: "AI Spawn Points",
            validate: validate_spawn_points,
        })
        .register_validation(ValidationRule {
            name: "AI Waypoints",
            validate: validate_waypoints,
        })
        .run();
}

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
    gizmos.line(pos + Vec3::Y * 1.0, pos + Vec3::new(0.15, 0.7, 0.0), color);
    gizmos.line(pos + Vec3::Y * 1.0, pos + Vec3::new(-0.15, 0.7, 0.0), color);
}

fn draw_waypoint_gizmo(gizmos: &mut Gizmos, transform: &GlobalTransform) {
    let pos = transform.translation();
    let color = Color::srgb(1.0, 0.6, 0.0);
    let size = 0.4;
    // Diamond shape
    gizmos.circle(Isometry3d::new(pos, Quat::IDENTITY), size, color);
    gizmos.circle(
        Isometry3d::new(pos, Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        size,
        color,
    );
    // Crosshair
    gizmos.line(pos + Vec3::X * size, pos - Vec3::X * size, color);
    gizmos.line(pos + Vec3::Z * size, pos - Vec3::Z * size, color);
}

fn regenerate_spawn_point(world: &mut World, entity: Entity) {
    if world.get::<Visibility>(entity).is_some() {
        return;
    }
    if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
        entity_mut.insert((Visibility::default(), Collider::sphere(0.3)));
    }
}

fn regenerate_waypoint(world: &mut World, entity: Entity) {
    if world.get::<Visibility>(entity).is_some() {
        return;
    }
    if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
        entity_mut.insert((Visibility::default(), Collider::sphere(0.3)));
    }
}

fn validate_spawn_points(world: &mut World) -> Vec<ValidationMessage> {
    let mut q = world.query_filtered::<Entity, With<SpawnPoint>>();
    let count = q.iter(world).count();
    if count == 0 {
        vec![ValidationMessage {
            severity: ValidationSeverity::Warning,
            message: "No Spawn Point in scene. AI agents need at least one.".into(),
            entity: None,
        }]
    } else {
        vec![]
    }
}

fn validate_waypoints(world: &mut World) -> Vec<ValidationMessage> {
    let mut q = world.query_filtered::<Entity, With<Waypoint>>();
    let count = q.iter(world).count();
    if count == 0 {
        vec![ValidationMessage {
            severity: ValidationSeverity::Warning,
            message: "No Waypoint in scene. Agents need targets to navigate toward.".into(),
            entity: None,
        }]
    } else {
        vec![]
    }
}
