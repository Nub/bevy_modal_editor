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

/// Helper: load a texture with repeat wrapping
fn load_repeating_texture(
    asset_server: &AssetServer,
    path: &str,
) -> Handle<Image> {
    asset_server.load_with_settings(
        path.to_string(),
        |s: &mut bevy::image::ImageLoaderSettings| {
            s.sampler = bevy::image::ImageSampler::Descriptor(
                bevy::image::ImageSamplerDescriptor {
                    address_mode_u: bevy::image::ImageAddressMode::Repeat,
                    address_mode_v: bevy::image::ImageAddressMode::Repeat,
                    ..default()
                },
            );
        },
    )
}

/// Helper: spawn a textured static cube entity
fn spawn_textured_cube(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    name: &str,
    transform: Transform,
    albedo: &str,
    normal: &str,
    uv_scale: [f32; 2],
    base_color: Color,
) {
    commands.spawn((
        bevy_modal_editor::SceneEntity,
        Name::new(name.to_string()),
        bevy_modal_editor::PrimitiveMarker {
            shape: bevy_modal_editor::PrimitiveShape::Cube,
        },
        MaterialRef::Inline(MaterialDefinition {
            base: BaseMaterialProps {
                base_color,
                base_color_texture: Some(albedo.into()),
                normal_map_texture: Some(normal.into()),
                uv_scale,
                ..default()
            },
            extension: None,
        }),
        Mesh3d(meshes.add(
            Mesh::from(Cuboid::new(1.0, 1.0, 1.0))
                .with_generated_tangents()
                .unwrap(),
        )),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color,
            base_color_texture: Some(load_repeating_texture(asset_server, albedo)),
            normal_map_texture: Some(load_repeating_texture(asset_server, normal)),
            uv_transform: bevy::math::Affine2::from_scale(Vec2::new(uv_scale[0], uv_scale[1])),
            ..default()
        })),
        transform,
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
    ));
}

/// Helper: spawn a textured static cylinder entity
fn spawn_textured_cylinder(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    asset_server: &AssetServer,
    name: &str,
    transform: Transform,
    albedo: &str,
    normal: &str,
    uv_scale: [f32; 2],
    base_color: Color,
) {
    commands.spawn((
        bevy_modal_editor::SceneEntity,
        Name::new(name.to_string()),
        bevy_modal_editor::PrimitiveMarker {
            shape: bevy_modal_editor::PrimitiveShape::Cylinder,
        },
        MaterialRef::Inline(MaterialDefinition {
            base: BaseMaterialProps {
                base_color,
                base_color_texture: Some(albedo.into()),
                normal_map_texture: Some(normal.into()),
                uv_scale,
                ..default()
            },
            extension: None,
        }),
        Mesh3d(meshes.add(
            Mesh::from(Cylinder::new(0.5, 1.0))
                .with_generated_tangents()
                .unwrap(),
        )),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color,
            base_color_texture: Some(load_repeating_texture(asset_server, albedo)),
            normal_map_texture: Some(load_repeating_texture(asset_server, normal)),
            uv_transform: bevy::math::Affine2::from_scale(Vec2::new(uv_scale[0], uv_scale[1])),
            ..default()
        })),
        transform,
        RigidBody::Static,
        Collider::cylinder(0.5, 1.0),
    ));
}

// Texture path constants
const STONE_ALBEDO: &str = "textures/stone_albedo.png";
const STONE_NORMAL: &str = "textures/stone_normal.png";
const METAL_ALBEDO: &str = "textures/metal_albedo.png";
const METAL_NORMAL: &str = "textures/metal_normal.png";
const WOOD_ALBEDO: &str = "textures/wood_albedo.png";
const WOOD_NORMAL: &str = "textures/wood_normal.png";

/// Setup the "Gauntlet" obstacle course level
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

    let cmd = &mut commands;
    let m = &mut *meshes;
    let mat = &mut *materials;
    let srv = &*asset_server;

    // =========================================================================
    // Ground & Boundaries
    // =========================================================================

    // Main floor: stone-textured ground (40 x 1 x 50)
    spawn_textured_cube(
        cmd, m, mat, srv,
        "Ground",
        Transform::from_translation(Vec3::new(0.0, -0.5, 0.0))
            .with_scale(Vec3::new(40.0, 1.0, 50.0)),
        STONE_ALBEDO, STONE_NORMAL,
        [8.0, 8.0],
        Color::WHITE,
    );

    // Pit: remove a section of floor for the bridge area (z=-6 to z=-12)
    // We'll build the floor in two halves with a gap
    // South floor section (z=6 to z=25) - already covered by main floor overlap
    // Actually, let's use a "pit floor" lower down to create a visible gap
    spawn_textured_cube(
        cmd, m, mat, srv,
        "Pit Floor",
        Transform::from_translation(Vec3::new(0.0, -4.0, -9.0))
            .with_scale(Vec3::new(40.0, 1.0, 8.0)),
        STONE_ALBEDO, STONE_NORMAL,
        [8.0, 2.0],
        Color::srgb(0.4, 0.4, 0.45),
    );

    // Cut the main floor by placing two floor sections with a gap instead
    // We need to remove the main ground and replace with two pieces.
    // Actually, let's just leave the main floor and add invisible "kill zone" walls
    // to fall through. Simpler: carve the pit by adding walls on the sides.

    // Perimeter walls (metal-textured, tall)
    let wall_color = Color::srgb(0.6, 0.6, 0.65);
    for (pos, scale, name) in [
        // Left wall
        (Vec3::new(-20.0, 2.0, 0.0), Vec3::new(0.5, 5.0, 50.0), "Wall Left"),
        // Right wall
        (Vec3::new(20.0, 2.0, 0.0), Vec3::new(0.5, 5.0, 50.0), "Wall Right"),
        // Back wall (north)
        (Vec3::new(0.0, 2.0, -25.0), Vec3::new(40.0, 5.0, 0.5), "Wall Back"),
        // Front wall (south)
        (Vec3::new(0.0, 2.0, 25.0), Vec3::new(40.0, 5.0, 0.5), "Wall Front"),
    ] {
        spawn_textured_cube(
            cmd, m, mat, srv,
            name,
            Transform::from_translation(pos).with_scale(scale),
            METAL_ALBEDO, METAL_NORMAL,
            [4.0, 2.0],
            wall_color,
        );
    }

    // =========================================================================
    // Spawn Area (south, z=20)
    // =========================================================================

    // Spawn platform: elevated wooden platform
    spawn_textured_cube(
        cmd, m, mat, srv,
        "Spawn Platform",
        Transform::from_translation(Vec3::new(0.0, 2.75, 20.0))
            .with_scale(Vec3::new(6.0, 0.5, 6.0)),
        WOOD_ALBEDO, WOOD_NORMAL,
        [2.0, 2.0],
        Color::srgb(0.85, 0.7, 0.5),
    );

    // Spawn Point (above platform)
    cmd.spawn((
        bevy_modal_editor::SceneEntity,
        SpawnPoint,
        Name::new("Spawn Point"),
        Transform::from_translation(Vec3::new(0.0, 4.0, 20.0)),
        Visibility::default(),
        Collider::sphere(0.3),
    ));

    // Entry ramp: wood ramp angled down from spawn platform to ground level
    // Ramp goes from y=3 at z=17 down to y=0 at z=13
    // Length along slope: sqrt(4^2 + 3^2) = 5, angle = atan(3/4) ≈ 0.6435 rad
    let ramp_angle = (3.0_f32 / 4.0).atan();
    let ramp_length = (4.0_f32 * 4.0 + 3.0 * 3.0).sqrt();
    spawn_textured_cube(
        cmd, m, mat, srv,
        "Entry Ramp",
        Transform::from_translation(Vec3::new(0.0, 1.5, 15.0))
            .with_rotation(Quat::from_rotation_x(ramp_angle))
            .with_scale(Vec3::new(4.0, 0.3, ramp_length)),
        WOOD_ALBEDO, WOOD_NORMAL,
        [3.0, 3.0],
        Color::srgb(0.75, 0.55, 0.35),
    );

    // Side rails for the entry ramp
    for (x, name) in [(-2.2, "Entry Ramp Rail Left"), (2.2, "Entry Ramp Rail Right")] {
        spawn_textured_cube(
            cmd, m, mat, srv,
            name,
            Transform::from_translation(Vec3::new(x, 2.0, 15.0))
                .with_rotation(Quat::from_rotation_x(ramp_angle))
                .with_scale(Vec3::new(0.2, 1.0, ramp_length)),
            METAL_ALBEDO, METAL_NORMAL,
            [1.0, 2.0],
            Color::srgb(0.5, 0.5, 0.55),
        );
    }

    // =========================================================================
    // Section 1: Narrow Corridor (z=12 to z=6)
    // =========================================================================

    let corridor_wall_color = Color::srgb(0.5, 0.5, 0.55);

    // Left corridor wall
    spawn_textured_cube(
        cmd, m, mat, srv,
        "Corridor Wall Left",
        Transform::from_translation(Vec3::new(-3.0, 1.0, 9.0))
            .with_scale(Vec3::new(0.5, 2.5, 7.0)),
        METAL_ALBEDO, METAL_NORMAL,
        [1.0, 2.0],
        corridor_wall_color,
    );

    // Right corridor wall
    spawn_textured_cube(
        cmd, m, mat, srv,
        "Corridor Wall Right",
        Transform::from_translation(Vec3::new(3.0, 1.0, 9.0))
            .with_scale(Vec3::new(0.5, 2.5, 7.0)),
        METAL_ALBEDO, METAL_NORMAL,
        [1.0, 2.0],
        corridor_wall_color,
    );

    // =========================================================================
    // Section 2: Pillar Weave (z=4 to z=-4)
    // =========================================================================

    let pillar_color = Color::srgb(0.4, 0.4, 0.45);
    let pillar_positions = [
        // Staggered grid: row 1 (z=3), row 2 (z=0), row 3 (z=-3)
        (Vec3::new(-4.0, 1.5, 3.0), "Pillar 1"),
        (Vec3::new(4.0, 1.5, 3.0), "Pillar 2"),
        (Vec3::new(0.0, 1.5, 0.0), "Pillar 3"),
        (Vec3::new(-6.0, 1.5, 0.0), "Pillar 4"),
        (Vec3::new(6.0, 1.5, 0.0), "Pillar 5"),
        (Vec3::new(-3.0, 1.5, -3.0), "Pillar 6"),
        (Vec3::new(3.0, 1.5, -3.0), "Pillar 7"),
    ];

    for (pos, name) in pillar_positions {
        spawn_textured_cylinder(
            cmd, m, mat, srv,
            name,
            Transform::from_translation(pos)
                .with_scale(Vec3::new(1.5, 3.0, 1.5)),
            METAL_ALBEDO, METAL_NORMAL,
            [2.0, 2.0],
            pillar_color,
        );
    }

    // =========================================================================
    // Section 3: Bridge over Gap (z=-6 to z=-12)
    // =========================================================================

    // Bridge: narrow wooden plank spanning the gap
    spawn_textured_cube(
        cmd, m, mat, srv,
        "Bridge",
        Transform::from_translation(Vec3::new(0.0, 2.0, -9.0))
            .with_scale(Vec3::new(3.0, 0.3, 8.0)),
        WOOD_ALBEDO, WOOD_NORMAL,
        [1.0, 3.0],
        Color::srgb(0.7, 0.5, 0.3),
    );

    // Bridge approach ramp (south side: ground level to bridge height)
    let bridge_ramp_angle = (2.0_f32 / 3.0).atan();
    let bridge_ramp_len = (3.0_f32 * 3.0 + 2.0 * 2.0).sqrt();
    spawn_textured_cube(
        cmd, m, mat, srv,
        "Bridge Ramp South",
        Transform::from_translation(Vec3::new(0.0, 1.0, -4.0))
            .with_rotation(Quat::from_rotation_x(-bridge_ramp_angle))
            .with_scale(Vec3::new(3.0, 0.3, bridge_ramp_len)),
        WOOD_ALBEDO, WOOD_NORMAL,
        [1.0, 2.0],
        Color::srgb(0.7, 0.5, 0.3),
    );

    // Bridge approach ramp (north side: bridge height down to ground)
    spawn_textured_cube(
        cmd, m, mat, srv,
        "Bridge Ramp North",
        Transform::from_translation(Vec3::new(0.0, 1.0, -14.0))
            .with_rotation(Quat::from_rotation_x(bridge_ramp_angle))
            .with_scale(Vec3::new(3.0, 0.3, bridge_ramp_len)),
        WOOD_ALBEDO, WOOD_NORMAL,
        [1.0, 2.0],
        Color::srgb(0.7, 0.5, 0.3),
    );

    // Low metal rails along the bridge
    for (x, name) in [(-1.7, "Bridge Rail Left"), (1.7, "Bridge Rail Right")] {
        spawn_textured_cube(
            cmd, m, mat, srv,
            name,
            Transform::from_translation(Vec3::new(x, 2.5, -9.0))
                .with_scale(Vec3::new(0.15, 0.8, 8.0)),
            METAL_ALBEDO, METAL_NORMAL,
            [1.0, 2.0],
            Color::srgb(0.5, 0.5, 0.55),
        );
    }

    // =========================================================================
    // Section 4: Final Ascent to Goal (z=-16 to z=-22)
    // =========================================================================

    // Long ascending ramp to goal platform
    // From ground (y=0) at z=-16 to y=4 at z=-20
    let final_ramp_angle = (4.0_f32 / 5.0).atan();
    let final_ramp_len = (5.0_f32 * 5.0 + 4.0 * 4.0).sqrt();
    spawn_textured_cube(
        cmd, m, mat, srv,
        "Goal Ramp",
        Transform::from_translation(Vec3::new(0.0, 2.0, -18.0))
            .with_rotation(Quat::from_rotation_x(-final_ramp_angle))
            .with_scale(Vec3::new(5.0, 0.3, final_ramp_len)),
        WOOD_ALBEDO, WOOD_NORMAL,
        [3.0, 3.0],
        Color::srgb(0.75, 0.55, 0.35),
    );

    // Side rails for final ramp
    for (x, name) in [(-2.8, "Goal Ramp Rail Left"), (2.8, "Goal Ramp Rail Right")] {
        spawn_textured_cube(
            cmd, m, mat, srv,
            name,
            Transform::from_translation(Vec3::new(x, 2.8, -18.0))
                .with_rotation(Quat::from_rotation_x(-final_ramp_angle))
                .with_scale(Vec3::new(0.2, 1.2, final_ramp_len)),
            METAL_ALBEDO, METAL_NORMAL,
            [1.0, 2.0],
            Color::srgb(0.5, 0.5, 0.55),
        );
    }

    // Goal platform: elevated stone platform
    spawn_textured_cube(
        cmd, m, mat, srv,
        "Goal Platform",
        Transform::from_translation(Vec3::new(0.0, 3.75, -22.0))
            .with_scale(Vec3::new(8.0, 0.5, 6.0)),
        STONE_ALBEDO, STONE_NORMAL,
        [2.0, 2.0],
        Color::srgb(0.9, 0.85, 0.8),
    );

    // Goal zone: semi-transparent green zone on the platform
    let goal_color = Color::srgba(0.2, 1.0, 0.3, 0.3);
    cmd.spawn((
        bevy_modal_editor::SceneEntity,
        GoalZone,
        Name::new("Goal Zone"),
        Mesh3d(m.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(mat.add(StandardMaterial {
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
        Transform::from_translation(Vec3::new(0.0, 5.0, -22.0))
            .with_scale(Vec3::new(3.0, 2.0, 3.0)),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
        Sensor,
    ));

    // =========================================================================
    // Lighting
    // =========================================================================

    cmd.spawn((
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
        Transform::from_translation(Vec3::new(10.0, 30.0, 10.0))
            .looking_at(Vec3::ZERO, Vec3::Y),
        Visibility::default(),
    ));

    info!("Gauntlet obstacle course level created");
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
