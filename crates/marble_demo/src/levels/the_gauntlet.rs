//! Level 1: The Gauntlet - Multi-section obstacle course

use super::level_gen::{spawn_cube, spawn_cylinder};
use bevy::prelude::*;

use crate::{GoalZone, SpawnPoint};

/// Build the Gauntlet level — serializable components only.
/// Runtime components (Mesh3d, Collider, etc.) are regenerated on load.
pub fn build_the_gauntlet(world: &mut World) {
    let stone = ("textures/stone_albedo.png", "textures/stone_normal.png");
    let metal = ("textures/metal_albedo.png", "textures/metal_normal.png");
    let wood = ("textures/wood_albedo.png", "textures/wood_normal.png");

    // =========================================================================
    // Ground & Boundaries
    // =========================================================================

    spawn_cube(
        world,
        "Ground",
        Transform::from_translation(Vec3::new(0.0, -0.5, 0.0))
            .with_scale(Vec3::new(40.0, 1.0, 50.0)),
        stone.0,
        stone.1,
        [8.0, 8.0],
        Color::WHITE,
    );

    // Pit floor below the bridge area
    spawn_cube(
        world,
        "Pit Floor",
        Transform::from_translation(Vec3::new(0.0, -4.0, -9.0))
            .with_scale(Vec3::new(40.0, 1.0, 8.0)),
        stone.0,
        stone.1,
        [8.0, 2.0],
        Color::srgb(0.4, 0.4, 0.45),
    );

    // Perimeter walls
    let wall_color = Color::srgb(0.6, 0.6, 0.65);
    for (pos, scale, name) in [
        (
            Vec3::new(-20.0, 2.0, 0.0),
            Vec3::new(0.5, 5.0, 50.0),
            "Wall Left",
        ),
        (
            Vec3::new(20.0, 2.0, 0.0),
            Vec3::new(0.5, 5.0, 50.0),
            "Wall Right",
        ),
        (
            Vec3::new(0.0, 2.0, -25.0),
            Vec3::new(40.0, 5.0, 0.5),
            "Wall Back",
        ),
        (
            Vec3::new(0.0, 2.0, 25.0),
            Vec3::new(40.0, 5.0, 0.5),
            "Wall Front",
        ),
    ] {
        spawn_cube(
            world,
            name,
            Transform::from_translation(pos).with_scale(scale),
            metal.0,
            metal.1,
            [4.0, 2.0],
            wall_color,
        );
    }

    // =========================================================================
    // Spawn Area (south, z=20)
    // =========================================================================

    spawn_cube(
        world,
        "Spawn Platform",
        Transform::from_translation(Vec3::new(0.0, 2.75, 20.0))
            .with_scale(Vec3::new(6.0, 0.5, 6.0)),
        wood.0,
        wood.1,
        [2.0, 2.0],
        Color::srgb(0.85, 0.7, 0.5),
    );

    // Spawn Point
    world.spawn((
        bevy_modal_editor::SceneEntity,
        SpawnPoint,
        Name::new("Spawn Point"),
        Transform::from_translation(Vec3::new(0.0, 4.0, 20.0)),
    ));

    // Entry ramp
    let ramp_angle = (3.0_f32 / 4.0).atan();
    let ramp_length = (4.0_f32 * 4.0 + 3.0 * 3.0).sqrt();
    spawn_cube(
        world,
        "Entry Ramp",
        Transform::from_translation(Vec3::new(0.0, 1.5, 15.0))
            .with_rotation(Quat::from_rotation_x(ramp_angle))
            .with_scale(Vec3::new(4.0, 0.3, ramp_length)),
        wood.0,
        wood.1,
        [3.0, 3.0],
        Color::srgb(0.75, 0.55, 0.35),
    );

    // Side rails for entry ramp
    for (x, name) in [(-2.2, "Entry Ramp Rail Left"), (2.2, "Entry Ramp Rail Right")] {
        spawn_cube(
            world,
            name,
            Transform::from_translation(Vec3::new(x, 2.0, 15.0))
                .with_rotation(Quat::from_rotation_x(ramp_angle))
                .with_scale(Vec3::new(0.2, 1.0, ramp_length)),
            metal.0,
            metal.1,
            [1.0, 2.0],
            Color::srgb(0.5, 0.5, 0.55),
        );
    }

    // =========================================================================
    // Section 1: Narrow Corridor (z=12 to z=6)
    // =========================================================================

    let corridor_wall_color = Color::srgb(0.5, 0.5, 0.55);

    spawn_cube(
        world,
        "Corridor Wall Left",
        Transform::from_translation(Vec3::new(-3.0, 1.0, 9.0))
            .with_scale(Vec3::new(0.5, 2.5, 7.0)),
        metal.0,
        metal.1,
        [1.0, 2.0],
        corridor_wall_color,
    );

    spawn_cube(
        world,
        "Corridor Wall Right",
        Transform::from_translation(Vec3::new(3.0, 1.0, 9.0))
            .with_scale(Vec3::new(0.5, 2.5, 7.0)),
        metal.0,
        metal.1,
        [1.0, 2.0],
        corridor_wall_color,
    );

    // =========================================================================
    // Section 2: Pillar Weave (z=4 to z=-4)
    // =========================================================================

    let pillar_color = Color::srgb(0.4, 0.4, 0.45);
    let pillar_positions = [
        (Vec3::new(-4.0, 1.5, 3.0), "Pillar 1"),
        (Vec3::new(4.0, 1.5, 3.0), "Pillar 2"),
        (Vec3::new(0.0, 1.5, 0.0), "Pillar 3"),
        (Vec3::new(-6.0, 1.5, 0.0), "Pillar 4"),
        (Vec3::new(6.0, 1.5, 0.0), "Pillar 5"),
        (Vec3::new(-3.0, 1.5, -3.0), "Pillar 6"),
        (Vec3::new(3.0, 1.5, -3.0), "Pillar 7"),
    ];

    for (pos, name) in pillar_positions {
        spawn_cylinder(
            world,
            name,
            Transform::from_translation(pos).with_scale(Vec3::new(1.5, 3.0, 1.5)),
            metal.0,
            metal.1,
            [2.0, 2.0],
            pillar_color,
        );
    }

    // =========================================================================
    // Section 3: Bridge over Gap (z=-6 to z=-12)
    // =========================================================================

    spawn_cube(
        world,
        "Bridge",
        Transform::from_translation(Vec3::new(0.0, 2.0, -9.0))
            .with_scale(Vec3::new(3.0, 0.3, 8.0)),
        wood.0,
        wood.1,
        [1.0, 3.0],
        Color::srgb(0.7, 0.5, 0.3),
    );

    let bridge_ramp_angle = (2.0_f32 / 3.0).atan();
    let bridge_ramp_len = (3.0_f32 * 3.0 + 2.0 * 2.0).sqrt();

    spawn_cube(
        world,
        "Bridge Ramp South",
        Transform::from_translation(Vec3::new(0.0, 1.0, -4.0))
            .with_rotation(Quat::from_rotation_x(-bridge_ramp_angle))
            .with_scale(Vec3::new(3.0, 0.3, bridge_ramp_len)),
        wood.0,
        wood.1,
        [1.0, 2.0],
        Color::srgb(0.7, 0.5, 0.3),
    );

    spawn_cube(
        world,
        "Bridge Ramp North",
        Transform::from_translation(Vec3::new(0.0, 1.0, -14.0))
            .with_rotation(Quat::from_rotation_x(bridge_ramp_angle))
            .with_scale(Vec3::new(3.0, 0.3, bridge_ramp_len)),
        wood.0,
        wood.1,
        [1.0, 2.0],
        Color::srgb(0.7, 0.5, 0.3),
    );

    // Bridge rails
    for (x, name) in [(-1.7, "Bridge Rail Left"), (1.7, "Bridge Rail Right")] {
        spawn_cube(
            world,
            name,
            Transform::from_translation(Vec3::new(x, 2.5, -9.0))
                .with_scale(Vec3::new(0.15, 0.8, 8.0)),
            metal.0,
            metal.1,
            [1.0, 2.0],
            Color::srgb(0.5, 0.5, 0.55),
        );
    }

    // =========================================================================
    // Section 4: Final Ascent to Goal (z=-16 to z=-22)
    // =========================================================================

    let final_ramp_angle = (4.0_f32 / 5.0).atan();
    let final_ramp_len = (5.0_f32 * 5.0 + 4.0 * 4.0).sqrt();

    spawn_cube(
        world,
        "Goal Ramp",
        Transform::from_translation(Vec3::new(0.0, 2.0, -18.0))
            .with_rotation(Quat::from_rotation_x(-final_ramp_angle))
            .with_scale(Vec3::new(5.0, 0.3, final_ramp_len)),
        wood.0,
        wood.1,
        [3.0, 3.0],
        Color::srgb(0.75, 0.55, 0.35),
    );

    for (x, name) in [(-2.8, "Goal Ramp Rail Left"), (2.8, "Goal Ramp Rail Right")] {
        spawn_cube(
            world,
            name,
            Transform::from_translation(Vec3::new(x, 2.8, -18.0))
                .with_rotation(Quat::from_rotation_x(-final_ramp_angle))
                .with_scale(Vec3::new(0.2, 1.2, final_ramp_len)),
            metal.0,
            metal.1,
            [1.0, 2.0],
            Color::srgb(0.5, 0.5, 0.55),
        );
    }

    // Goal platform
    spawn_cube(
        world,
        "Goal Platform",
        Transform::from_translation(Vec3::new(0.0, 3.75, -22.0))
            .with_scale(Vec3::new(8.0, 0.5, 6.0)),
        stone.0,
        stone.1,
        [2.0, 2.0],
        Color::srgb(0.9, 0.85, 0.8),
    );

    // Goal zone (serializable marker only — Mesh3d, Collider, Sensor regenerated)
    world.spawn((
        bevy_modal_editor::SceneEntity,
        GoalZone,
        Name::new("Goal Zone"),
        bevy_editor_game::MaterialRef::Inline(bevy_editor_game::MaterialDefinition {
            base: bevy_editor_game::BaseMaterialProps {
                base_color: Color::srgba(0.2, 1.0, 0.3, 0.3),
                alpha_mode: bevy_editor_game::AlphaModeValue::Blend,
                ..default()
            },
            extension: None,
        }),
        Transform::from_translation(Vec3::new(0.0, 5.0, -22.0))
            .with_scale(Vec3::new(3.0, 2.0, 3.0)),
    ));

    // =========================================================================
    // Lighting
    // =========================================================================

    world.spawn((
        bevy_modal_editor::SceneEntity,
        Name::new("Sun"),
        bevy_modal_editor::DirectionalLightMarker {
            color: Color::WHITE,
            illuminance: 15000.0,
            shadows_enabled: true,
        },
        Transform::from_translation(Vec3::new(10.0, 30.0, 10.0))
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));

    info!("Built The Gauntlet level");
}
