//! Marble Demo - A marble rolling game using bevy_modal_editor
//!
//! Demonstrates the editor's play/pause/reset lifecycle:
//! 1. Design a level in the editor (ground, ramps, obstacles, spawn point, goal zone)
//! 2. Press F5 to play — marble spawns at SpawnPoint, camera follows
//! 3. WASD to roll the marble, Space to jump
//! 4. Reach the GoalZone to complete the level
//! 5. F6 to pause, F5 to resume, F7 to reset

mod checkerboard;
mod game_camera;
mod marble;
mod timer;

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_editor_game::{
    AlphaModeValue, BaseMaterialProps, CustomEntityType, MaterialDefinition, MaterialRef,
    RegisterCustomEntityExt, RegisterValidationExt, ValidationMessage, ValidationRule,
    ValidationSeverity,
};
use bevy_modal_editor::materials::RegisterMaterialTypeExt;
use bevy_modal_editor::{EditorPlugin, EditorPluginConfig, GamePlugin};

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
        .add_plugins(DefaultPlugins)
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
        .add_systems(Startup, setup_default_level)
        .run();
}

/// Setup a default marble demo level if the scene is empty
fn setup_default_level(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    existing: Query<Entity, With<bevy_modal_editor::SceneEntity>>,
) {
    // Don't spawn if scene already has entities (e.g. loaded from file)
    if existing.iter().count() > 0 {
        return;
    }

    // Ground plane (marble texture)
    let ground_color = Color::WHITE;
    commands.spawn((
        bevy_modal_editor::SceneEntity,
        Name::new("Ground"),
        bevy_modal_editor::PrimitiveMarker {
            shape: bevy_modal_editor::PrimitiveShape::Cube,
        },
        MaterialRef::Inline(MaterialDefinition {
            base: BaseMaterialProps {
                base_color: ground_color,
                base_color_texture: Some("textures/brick_albedo.png".into()),
                normal_map_texture: Some("textures/brick_normal.png".into()),
                uv_scale: [6.0, 6.0],
                ..default()
            },
            extension: None,
        }),
        Mesh3d(meshes.add(Mesh::from(Cuboid::new(1.0, 1.0, 1.0)).with_generated_tangents().unwrap())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: ground_color,
            base_color_texture: Some(asset_server.load_with_settings(
                "textures/brick_albedo.png",
                |s: &mut bevy::image::ImageLoaderSettings| {
                    s.sampler = bevy::image::ImageSampler::Descriptor(
                        bevy::image::ImageSamplerDescriptor {
                            address_mode_u: bevy::image::ImageAddressMode::Repeat,
                            address_mode_v: bevy::image::ImageAddressMode::Repeat,
                            ..default()
                        },
                    );
                },
            )),
            normal_map_texture: Some(asset_server.load_with_settings(
                "textures/brick_normal.png",
                |s: &mut bevy::image::ImageLoaderSettings| {
                    s.sampler = bevy::image::ImageSampler::Descriptor(
                        bevy::image::ImageSamplerDescriptor {
                            address_mode_u: bevy::image::ImageAddressMode::Repeat,
                            address_mode_v: bevy::image::ImageAddressMode::Repeat,
                            ..default()
                        },
                    );
                },
            )),
            uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(6.0)),
            ..default()
        })),
        Transform::from_translation(Vec3::new(0.0, -0.5, 0.0))
            .with_scale(Vec3::new(30.0, 1.0, 30.0)),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
    ));

    // Ramp
    let ramp_color = Color::srgb(0.6, 0.4, 0.3);
    commands.spawn((
        bevy_modal_editor::SceneEntity,
        Name::new("Ramp"),
        bevy_modal_editor::PrimitiveMarker {
            shape: bevy_modal_editor::PrimitiveShape::Cube,
        },
        MaterialRef::Inline(MaterialDefinition::standard(ramp_color)),
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: ramp_color,
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
        let wall_color = Color::srgb(0.3, 0.3, 0.35);
        commands.spawn((
            bevy_modal_editor::SceneEntity,
            Name::new(name),
            bevy_modal_editor::PrimitiveMarker {
                shape: bevy_modal_editor::PrimitiveShape::Cube,
            },
            MaterialRef::Inline(MaterialDefinition::standard(wall_color)),
            Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: wall_color,
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
    let goal_color = Color::srgba(0.2, 1.0, 0.3, 0.3);
    commands.spawn((
        bevy_modal_editor::SceneEntity,
        GoalZone,
        Name::new("Goal Zone"),
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: goal_color,
            alpha_mode: AlphaMode::Blend,
            ..default()
        })),
        MaterialRef::Inline(MaterialDefinition {
            base: BaseMaterialProps {
                base_color: goal_color,
                alpha_mode: AlphaModeValue::Blend,
                ..default()
            },
            extension: None,
        }),
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
        let obstacle_color = Color::srgb(0.7, 0.3, 0.3);
        commands.spawn((
            bevy_modal_editor::SceneEntity,
            Name::new(format!("Obstacle {}", i + 1)),
            bevy_modal_editor::PrimitiveMarker {
                shape: bevy_modal_editor::PrimitiveShape::Cube,
            },
            MaterialRef::Inline(MaterialDefinition::standard(obstacle_color)),
            Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: obstacle_color,
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
