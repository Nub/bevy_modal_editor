//! Marble Demo - A marble rolling game using bevy_modal_editor
//!
//! Demonstrates the editor's play/pause/reset lifecycle:
//! 1. Design a level in the editor (ground, ramps, obstacles, spawn point, goal zone)
//! 2. Press F5 to play — marble spawns at SpawnPoint, camera follows
//! 3. WASD to roll the marble, Space to jump
//! 4. Reach the GoalZone to complete the level
//! 5. F6 to pause, F5 to resume, F7 to reset

mod game_camera;
mod marble;
mod timer;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_editor_game::{CustomEntityType, RegisterCustomEntityExt};
use bevy_modal_editor::{EditorPlugin, EditorPluginConfig, EditorState, GamePlugin};

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
        .add_plugins(DefaultPlugins)
        .add_plugins(EditorPlugin::new(EditorPluginConfig {
            pause_physics_on_startup: true,
            ..default()
        }))
        .add_plugins(GamePlugin)
        .add_plugins(marble::MarblePlugin)
        .add_plugins(game_camera::GameCameraPlugin)
        .add_plugins(timer::GameTimerPlugin)
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
                    ))
                    .id()
            },
        })
        .add_systems(
            Update,
            (
                draw_spawn_point_gizmos,
                draw_goal_zone_gizmos,
                regenerate_spawn_points,
                regenerate_goal_zones,
            ),
        )
        .add_systems(Startup, setup_default_level)
        .run();
}

/// Setup a default marble demo level if the scene is empty
fn setup_default_level(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing: Query<Entity, With<bevy_modal_editor::SceneEntity>>,
) {
    // Don't spawn if scene already has entities (e.g. loaded from file)
    if existing.iter().count() > 0 {
        return;
    }

    // Ground plane
    commands.spawn((
        bevy_modal_editor::SceneEntity,
        Name::new("Ground"),
        bevy_modal_editor::PrimitiveMarker {
            shape: bevy_modal_editor::PrimitiveShape::Cube,
        },
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.5, 0.5, 0.55),
            ..default()
        })),
        Transform::from_translation(Vec3::new(0.0, -0.5, 0.0))
            .with_scale(Vec3::new(30.0, 1.0, 30.0)),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
    ));

    // Ramp
    commands.spawn((
        bevy_modal_editor::SceneEntity,
        Name::new("Ramp"),
        bevy_modal_editor::PrimitiveMarker {
            shape: bevy_modal_editor::PrimitiveShape::Cube,
        },
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.4, 0.3),
            ..default()
        })),
        Transform::from_translation(Vec3::new(0.0, 0.5, -5.0))
            .with_rotation(Quat::from_rotation_x(-0.2))
            .with_scale(Vec3::new(4.0, 0.3, 8.0)),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
    ));

    // Walls
    for (pos, scale, name) in [
        (
            Vec3::new(-15.0, 1.0, 0.0),
            Vec3::new(0.5, 2.0, 30.0),
            "Wall Left",
        ),
        (
            Vec3::new(15.0, 1.0, 0.0),
            Vec3::new(0.5, 2.0, 30.0),
            "Wall Right",
        ),
        (
            Vec3::new(0.0, 1.0, -15.0),
            Vec3::new(30.0, 2.0, 0.5),
            "Wall Back",
        ),
        (
            Vec3::new(0.0, 1.0, 15.0),
            Vec3::new(30.0, 2.0, 0.5),
            "Wall Front",
        ),
    ] {
        commands.spawn((
            bevy_modal_editor::SceneEntity,
            Name::new(name),
            bevy_modal_editor::PrimitiveMarker {
                shape: bevy_modal_editor::PrimitiveShape::Cube,
            },
            Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.3, 0.3, 0.35),
                ..default()
            })),
            Transform::from_translation(pos).with_scale(scale),
            RigidBody::Static,
            Collider::cuboid(1.0, 1.0, 1.0),
        ));
    }

    // Spawn Point
    commands.spawn((
        bevy_modal_editor::SceneEntity,
        SpawnPoint,
        Name::new("Spawn Point"),
        Transform::from_translation(Vec3::new(0.0, 2.0, 10.0)),
        Visibility::default(),
        Collider::sphere(0.3),
    ));

    // Goal Zone
    commands.spawn((
        bevy_modal_editor::SceneEntity,
        GoalZone,
        Name::new("Goal Zone"),
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.2, 1.0, 0.3, 0.3),
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        Transform::from_translation(Vec3::new(0.0, 1.0, -12.0)).with_scale(Vec3::splat(3.0)),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
        Sensor,
    ));

    // Obstacles
    for (i, (pos, scale)) in [
        (Vec3::new(5.0, 0.5, 0.0), Vec3::new(2.0, 1.0, 2.0)),
        (Vec3::new(-5.0, 0.5, -3.0), Vec3::new(1.5, 1.0, 3.0)),
        (Vec3::new(3.0, 0.5, -8.0), Vec3::new(1.0, 1.0, 1.0)),
    ]
    .iter()
    .enumerate()
    {
        commands.spawn((
            bevy_modal_editor::SceneEntity,
            Name::new(format!("Obstacle {}", i + 1)),
            bevy_modal_editor::PrimitiveMarker {
                shape: bevy_modal_editor::PrimitiveShape::Cube,
            },
            Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: Color::srgb(0.7, 0.3, 0.3),
                ..default()
            })),
            Transform::from_translation(*pos).with_scale(*scale),
            RigidBody::Static,
            Collider::cuboid(1.0, 1.0, 1.0),
        ));
    }

    // Light
    commands.spawn((
        bevy_modal_editor::SceneEntity,
        Name::new("Sun"),
        bevy_modal_editor::DirectionalLightMarker {
            color: Color::WHITE,
            illuminance: 15000.0,
            shadows_enabled: true,
        },
        DirectionalLight {
            color: Color::WHITE,
            illuminance: 15000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_translation(Vec3::new(10.0, 20.0, 10.0)).looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::default(),
    ));

    info!("Default marble demo level created");
}

/// Draw gizmos for spawn points (upward arrow marker)
fn draw_spawn_point_gizmos(
    mut gizmos: Gizmos,
    spawn_points: Query<&GlobalTransform, With<SpawnPoint>>,
    editor_state: Res<EditorState>,
) {
    if !editor_state.gizmos_visible {
        return;
    }

    let color = Color::srgb(0.2, 0.8, 1.0);
    for transform in spawn_points.iter() {
        let pos = transform.translation();
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
}

/// Draw gizmos for goal zones (green wireframe cube)
fn draw_goal_zone_gizmos(
    mut gizmos: Gizmos,
    goal_zones: Query<&GlobalTransform, With<GoalZone>>,
    editor_state: Res<EditorState>,
) {
    if !editor_state.gizmos_visible {
        return;
    }

    let color = Color::srgb(0.2, 1.0, 0.3);
    for transform in goal_zones.iter() {
        let pos = transform.translation();
        let size = 0.5;
        // Wireframe cube via 3 circles
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
}

/// Regenerate runtime components for spawn points after scene restore
fn regenerate_spawn_points(
    mut commands: Commands,
    query: Query<Entity, (With<SpawnPoint>, Without<Visibility>)>,
) {
    for entity in &query {
        commands.entity(entity).insert((
            Visibility::default(),
            Collider::sphere(0.3),
        ));
    }
}

/// Regenerate runtime components for goal zones after scene restore
fn regenerate_goal_zones(
    mut commands: Commands,
    query: Query<Entity, (With<GoalZone>, Without<Visibility>)>,
) {
    for entity in &query {
        commands.entity(entity).insert((
            Visibility::default(),
            Collider::cuboid(1.0, 1.0, 1.0),
            Sensor,
        ));
    }
}
