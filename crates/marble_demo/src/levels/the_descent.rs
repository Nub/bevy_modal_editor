//! Level 3: The Descent - Multi-tier descent from high to low

use super::level_gen::{spawn_cube, spawn_cylinder};
use bevy::prelude::*;

use crate::{GoalZone, SpawnPoint};

/// Build the Descent level — serializable components only.
pub fn build_the_descent(world: &mut World) {
    let stone = ("textures/stone_albedo.png", "textures/stone_normal.png");
    let metal = ("textures/metal_albedo.png", "textures/metal_normal.png");
    let wood = ("textures/wood_albedo.png", "textures/wood_normal.png");

    // =========================================================================
    // Ground & Arena
    // =========================================================================

    // Ground floor with central pit
    spawn_cube(
        world,
        "Ground",
        Transform::from_translation(Vec3::new(0.0, -0.5, 0.0))
            .with_scale(Vec3::new(150.0, 1.0, 150.0)),
        stone.0,
        stone.1,
        [20.0, 20.0],
        Color::srgb(0.45, 0.45, 0.5),
    );

    // Shallow pit surrounding the goal
    spawn_cube(
        world,
        "Pit Floor",
        Transform::from_translation(Vec3::new(0.0, -15.0, 0.0))
            .with_scale(Vec3::new(30.0, 1.0, 30.0)),
        stone.0,
        stone.1,
        [6.0, 6.0],
        Color::srgb(0.35, 0.35, 0.4),
    );

    // Perimeter walls
    let wall_color = Color::srgb(0.55, 0.55, 0.6);
    for (pos, scale, name) in [
        (
            Vec3::new(-75.0, 10.0, 0.0),
            Vec3::new(0.5, 100.0, 150.0),
            "Wall West",
        ),
        (
            Vec3::new(75.0, 10.0, 0.0),
            Vec3::new(0.5, 100.0, 150.0),
            "Wall East",
        ),
        (
            Vec3::new(0.0, 10.0, -75.0),
            Vec3::new(150.0, 100.0, 0.5),
            "Wall North",
        ),
        (
            Vec3::new(0.0, 10.0, 75.0),
            Vec3::new(150.0, 100.0, 0.5),
            "Wall South",
        ),
    ] {
        spawn_cube(
            world,
            name,
            Transform::from_translation(pos).with_scale(scale),
            metal.0,
            metal.1,
            [10.0, 6.0],
            wall_color,
        );
    }

    // =========================================================================
    // Tier 0 (y=80): Starting Platform
    // =========================================================================

    spawn_cube(
        world,
        "Start Platform",
        Transform::from_translation(Vec3::new(0.0, 80.0, 0.0))
            .with_scale(Vec3::new(20.0, 1.0, 20.0)),
        stone.0,
        stone.1,
        [4.0, 4.0],
        Color::srgb(0.7, 0.7, 0.75),
    );

    world.spawn((
        bevy_modal_editor::SceneEntity,
        SpawnPoint,
        Name::new("Spawn Point"),
        Transform::from_translation(Vec3::new(0.0, 82.0, 0.0)),
    ));

    // =========================================================================
    // Ramp: Tier 0 → Tier 1 (y=80 → y=60)
    // =========================================================================

    // Long ramp with guardrails
    let drop1 = 20.0_f32;
    let run1 = 40.0_f32;
    let ramp1_len = (drop1 * drop1 + run1 * run1).sqrt();
    let ramp1_angle = (drop1 / run1).atan();

    spawn_cube(
        world,
        "Ramp T0-T1",
        Transform::from_translation(Vec3::new(0.0, 70.0, 25.0))
            .with_rotation(Quat::from_rotation_x(ramp1_angle))
            .with_scale(Vec3::new(6.0, 0.4, ramp1_len)),
        wood.0,
        wood.1,
        [2.0, 6.0],
        Color::srgb(0.75, 0.55, 0.35),
    );

    for (x, name) in [(-3.3, "Ramp T0-T1 Rail L"), (3.3, "Ramp T0-T1 Rail R")] {
        spawn_cube(
            world,
            name,
            Transform::from_translation(Vec3::new(x, 70.5, 25.0))
                .with_rotation(Quat::from_rotation_x(ramp1_angle))
                .with_scale(Vec3::new(0.2, 1.0, ramp1_len)),
            metal.0,
            metal.1,
            [1.0, 4.0],
            Color::srgb(0.5, 0.5, 0.55),
        );
    }

    // =========================================================================
    // Tier 1 (y=60): Maze Section
    // =========================================================================

    spawn_cube(
        world,
        "Tier 1 Platform",
        Transform::from_translation(Vec3::new(0.0, 59.5, 50.0))
            .with_scale(Vec3::new(40.0, 1.0, 30.0)),
        stone.0,
        stone.1,
        [8.0, 6.0],
        Color::srgb(0.65, 0.65, 0.7),
    );

    // Maze walls on tier 1
    let maze_color = Color::srgb(0.5, 0.5, 0.55);
    let maze_walls = [
        (Vec3::new(-10.0, 61.5, 45.0), Vec3::new(0.5, 3.0, 20.0), "Maze Wall 1"),
        (Vec3::new(10.0, 61.5, 55.0), Vec3::new(0.5, 3.0, 20.0), "Maze Wall 2"),
        (Vec3::new(0.0, 61.5, 50.0), Vec3::new(12.0, 3.0, 0.5), "Maze Wall 3"),
        (Vec3::new(-5.0, 61.5, 58.0), Vec3::new(10.0, 3.0, 0.5), "Maze Wall 4"),
    ];

    for (pos, scale, name) in maze_walls {
        spawn_cube(
            world,
            name,
            Transform::from_translation(pos).with_scale(scale),
            metal.0,
            metal.1,
            [2.0, 2.0],
            maze_color,
        );
    }

    // =========================================================================
    // Ramp: Tier 1 → Tier 2 (y=60 → y=40)
    // =========================================================================

    let drop2 = 20.0_f32;
    let run2 = 35.0_f32;
    let ramp2_len = (drop2 * drop2 + run2 * run2).sqrt();
    let ramp2_angle = (drop2 / run2).atan();

    spawn_cube(
        world,
        "Ramp T1-T2",
        Transform::from_translation(Vec3::new(25.0, 50.0, 55.0))
            .with_rotation(
                Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)
                    * Quat::from_rotation_x(ramp2_angle),
            )
            .with_scale(Vec3::new(5.0, 0.4, ramp2_len)),
        wood.0,
        wood.1,
        [2.0, 5.0],
        Color::srgb(0.75, 0.55, 0.35),
    );

    // =========================================================================
    // Tier 2 (y=40): Bridge Platforms
    // =========================================================================

    // Series of disconnected platforms with narrow bridges
    let tier2_platforms = [
        (Vec3::new(50.0, 39.5, 55.0), Vec3::new(12.0, 1.0, 12.0), "Island A"),
        (Vec3::new(50.0, 39.5, 30.0), Vec3::new(12.0, 1.0, 12.0), "Island B"),
        (Vec3::new(30.0, 39.5, 15.0), Vec3::new(12.0, 1.0, 12.0), "Island C"),
        (Vec3::new(10.0, 39.5, 0.0), Vec3::new(12.0, 1.0, 12.0), "Island D"),
    ];

    for (pos, scale, name) in tier2_platforms {
        spawn_cube(
            world,
            name,
            Transform::from_translation(pos).with_scale(scale),
            stone.0,
            stone.1,
            [3.0, 3.0],
            Color::srgb(0.6, 0.6, 0.65),
        );
    }

    // Narrow bridges between islands
    let bridges = [
        (
            Vec3::new(50.0, 40.0, 42.5),
            Vec3::new(3.0, 0.3, 13.0),
            Quat::IDENTITY,
            "Bridge A-B",
        ),
        (
            Vec3::new(40.0, 40.0, 22.5),
            Vec3::new(3.0, 0.3, 17.0),
            Quat::from_rotation_y(0.5),
            "Bridge B-C",
        ),
        (
            Vec3::new(20.0, 40.0, 7.5),
            Vec3::new(2.5, 0.3, 17.0),
            Quat::from_rotation_y(0.3),
            "Bridge C-D",
        ),
    ];

    for (pos, scale, rot, name) in bridges {
        spawn_cube(
            world,
            name,
            Transform::from_translation(pos)
                .with_rotation(rot)
                .with_scale(scale),
            wood.0,
            wood.1,
            [1.0, 4.0],
            Color::srgb(0.7, 0.5, 0.3),
        );
    }

    // =========================================================================
    // Ramp: Tier 2 → Tier 3 (y=40 → y=20)
    // =========================================================================

    let drop3 = 20.0_f32;
    let run3 = 30.0_f32;
    let ramp3_len = (drop3 * drop3 + run3 * run3).sqrt();
    let ramp3_angle = (drop3 / run3).atan();

    spawn_cube(
        world,
        "Ramp T2-T3",
        Transform::from_translation(Vec3::new(-5.0, 30.0, -10.0))
            .with_rotation(Quat::from_rotation_x(ramp3_angle))
            .with_scale(Vec3::new(5.0, 0.4, ramp3_len)),
        wood.0,
        wood.1,
        [2.0, 5.0],
        Color::srgb(0.75, 0.55, 0.35),
    );

    for (x, name) in [(-2.8, "Ramp T2-T3 Rail L"), (2.8, "Ramp T2-T3 Rail R")] {
        spawn_cube(
            world,
            name,
            Transform::from_translation(Vec3::new(-5.0 + x, 30.5, -10.0))
                .with_rotation(Quat::from_rotation_x(ramp3_angle))
                .with_scale(Vec3::new(0.2, 1.0, ramp3_len)),
            metal.0,
            metal.1,
            [1.0, 4.0],
            Color::srgb(0.5, 0.5, 0.55),
        );
    }

    // =========================================================================
    // Tier 3 (y=20): Slalom Ramp
    // =========================================================================

    // Long slalom ramp with staggered pillars
    spawn_cube(
        world,
        "Slalom Platform",
        Transform::from_translation(Vec3::new(-5.0, 19.5, -30.0))
            .with_scale(Vec3::new(20.0, 1.0, 50.0)),
        stone.0,
        stone.1,
        [4.0, 8.0],
        Color::srgb(0.6, 0.6, 0.65),
    );

    // Gentle slope down along the slalom
    let slalom_angle = (10.0_f32 / 50.0).atan();
    let slalom_len = (10.0_f32 * 10.0 + 50.0 * 50.0).sqrt();
    spawn_cube(
        world,
        "Slalom Ramp",
        Transform::from_translation(Vec3::new(-5.0, 15.0, -30.0))
            .with_rotation(Quat::from_rotation_x(slalom_angle))
            .with_scale(Vec3::new(16.0, 0.3, slalom_len)),
        wood.0,
        wood.1,
        [3.0, 8.0],
        Color::srgb(0.75, 0.55, 0.35),
    );

    // Staggered slalom pillars
    let pillar_color = Color::srgb(0.45, 0.45, 0.5);
    let pillar_positions = [
        Vec3::new(-10.0, 21.0, -20.0),
        Vec3::new(0.0, 21.0, -24.0),
        Vec3::new(-10.0, 20.0, -28.0),
        Vec3::new(0.0, 20.0, -32.0),
        Vec3::new(-10.0, 19.0, -36.0),
        Vec3::new(0.0, 19.0, -40.0),
        Vec3::new(-10.0, 18.0, -44.0),
        Vec3::new(0.0, 18.0, -48.0),
    ];

    for (i, pos) in pillar_positions.iter().enumerate() {
        spawn_cylinder(
            world,
            &format!("Slalom Pillar {}", i + 1),
            Transform::from_translation(*pos).with_scale(Vec3::new(2.0, 4.0, 2.0)),
            metal.0,
            metal.1,
            [2.0, 2.0],
            pillar_color,
        );
    }

    // =========================================================================
    // Final Ramp: Tier 3 → Ground (y=20 → y=0)
    // =========================================================================

    let drop4 = 20.0_f32;
    let run4 = 30.0_f32;
    let ramp4_len = (drop4 * drop4 + run4 * run4).sqrt();
    let ramp4_angle = (drop4 / run4).atan();

    spawn_cube(
        world,
        "Final Ramp",
        Transform::from_translation(Vec3::new(-5.0, 5.0, -65.0))
            .with_rotation(Quat::from_rotation_x(-ramp4_angle))
            .with_scale(Vec3::new(6.0, 0.4, ramp4_len)),
        wood.0,
        wood.1,
        [2.0, 5.0],
        Color::srgb(0.75, 0.55, 0.35),
    );

    // =========================================================================
    // Goal Area (ground level)
    // =========================================================================

    spawn_cube(
        world,
        "Goal Platform",
        Transform::from_translation(Vec3::new(0.0, 0.25, 0.0))
            .with_scale(Vec3::new(10.0, 0.5, 10.0)),
        stone.0,
        stone.1,
        [2.0, 2.0],
        Color::srgb(0.85, 0.8, 0.75),
    );

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
        Transform::from_translation(Vec3::new(0.0, 2.0, 0.0))
            .with_scale(Vec3::new(4.0, 3.0, 4.0)),
    ));

    // =========================================================================
    // Lighting
    // =========================================================================

    world.spawn((
        bevy_modal_editor::SceneEntity,
        Name::new("Sun"),
        bevy_modal_editor::DirectionalLightMarker {
            color: Color::WHITE,
            illuminance: 20000.0,
            shadows_enabled: true,
        },
        Transform::from_translation(Vec3::new(40.0, 100.0, 30.0))
            .looking_at(Vec3::new(0.0, 30.0, 0.0), Vec3::Y),
    ));

    info!("Built The Descent level");
}
