//! Level 2: The Spiral - Vertical tower with spiral ramp

use super::level_gen::{spawn_cube, spawn_cylinder};
use bevy::prelude::*;

use crate::{GoalZone, SpawnPoint};

/// Build the Spiral level — serializable components only.
pub fn build_the_spiral(world: &mut World) {
    let stone = ("textures/stone_albedo.png", "textures/stone_normal.png");
    let metal = ("textures/metal_albedo.png", "textures/metal_normal.png");
    let wood = ("textures/wood_albedo.png", "textures/wood_normal.png");
    let brick = ("textures/brick_albedo.png", "textures/brick_normal.png");

    // =========================================================================
    // Ground & Arena
    // =========================================================================

    // Ground floor
    spawn_cube(
        world,
        "Ground",
        Transform::from_translation(Vec3::new(0.0, -0.5, 0.0))
            .with_scale(Vec3::new(100.0, 1.0, 100.0)),
        stone.0,
        stone.1,
        [16.0, 16.0],
        Color::srgb(0.5, 0.5, 0.55),
    );

    // Perimeter walls
    let wall_color = Color::srgb(0.55, 0.55, 0.6);
    for (pos, scale, name) in [
        (
            Vec3::new(-50.0, 5.0, 0.0),
            Vec3::new(0.5, 10.0, 100.0),
            "Wall West",
        ),
        (
            Vec3::new(50.0, 5.0, 0.0),
            Vec3::new(0.5, 10.0, 100.0),
            "Wall East",
        ),
        (
            Vec3::new(0.0, 5.0, -50.0),
            Vec3::new(100.0, 10.0, 0.5),
            "Wall North",
        ),
        (
            Vec3::new(0.0, 5.0, 50.0),
            Vec3::new(100.0, 10.0, 0.5),
            "Wall South",
        ),
    ] {
        spawn_cube(
            world,
            name,
            Transform::from_translation(pos).with_scale(scale),
            metal.0,
            metal.1,
            [8.0, 4.0],
            wall_color,
        );
    }

    // =========================================================================
    // Central Column
    // =========================================================================

    spawn_cylinder(
        world,
        "Central Column",
        Transform::from_translation(Vec3::new(0.0, 45.0, 0.0))
            .with_scale(Vec3::new(30.0, 90.0, 30.0)),
        brick.0,
        brick.1,
        [8.0, 12.0],
        Color::srgb(0.7, 0.55, 0.4),
    );

    // =========================================================================
    // Bottom Platform (Spawn)
    // =========================================================================

    spawn_cube(
        world,
        "Bottom Platform",
        Transform::from_translation(Vec3::new(20.0, 2.0, 0.0))
            .with_scale(Vec3::new(15.0, 0.5, 15.0)),
        stone.0,
        stone.1,
        [4.0, 4.0],
        Color::srgb(0.7, 0.7, 0.75),
    );

    world.spawn((
        bevy_modal_editor::SceneEntity,
        SpawnPoint,
        Name::new("Spawn Point"),
        Transform::from_translation(Vec3::new(20.0, 4.0, 0.0)),
    ));

    // =========================================================================
    // Spiral Ramp Segments
    // =========================================================================

    // Each segment is a quarter-turn ramp around the central column.
    // The column has radius 15, ramps are placed at radius ~22.
    // Total height: from y=2 to y=85 (~83 units), 12 segments = ~6.9 per segment.

    let num_segments = 12;
    let start_y = 2.5;
    let rise_per_segment = 6.9;
    let ramp_radius = 22.0;
    let ramp_width = 8.0;
    let ramp_thickness = 0.4;

    for i in 0..num_segments {
        let base_y = start_y + i as f32 * rise_per_segment;
        let mid_y = base_y + rise_per_segment * 0.5;
        let angle = i as f32 * std::f32::consts::FRAC_PI_2; // 90 degrees per segment

        // Ramp direction vectors
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        let cos_b = (angle + std::f32::consts::FRAC_PI_2).cos();
        let sin_b = (angle + std::f32::consts::FRAC_PI_2).sin();

        // Ramp center position (midpoint between start and end of quarter turn)
        let mid_angle = angle + std::f32::consts::FRAC_PI_4;
        let cx = ramp_radius * mid_angle.cos();
        let cz = ramp_radius * mid_angle.sin();

        // Ramp length along the chord of the quarter circle
        let chord_len = ramp_radius * (2.0_f32).sqrt();
        let ramp_len = (chord_len * chord_len + rise_per_segment * rise_per_segment).sqrt();
        let slope_angle = (rise_per_segment / chord_len).atan();

        // Rotation: face along the chord direction, then tilt for slope
        let dir = Vec3::new(cos_b - cos_a, 0.0, sin_b - sin_a).normalize();
        let yaw = (-dir.x).atan2(-dir.z);

        // Progressive narrowing for difficulty
        let width = if i < 6 {
            ramp_width
        } else if i < 9 {
            ramp_width * 0.75
        } else {
            ramp_width * 0.6
        };

        spawn_cube(
            world,
            &format!("Ramp Segment {}", i + 1),
            Transform::from_translation(Vec3::new(cx, mid_y, cz))
                .with_rotation(
                    Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-slope_angle),
                )
                .with_scale(Vec3::new(width, ramp_thickness, ramp_len)),
            wood.0,
            wood.1,
            [2.0, 4.0],
            Color::srgb(0.8, 0.65, 0.45),
        );

        // Rest platform at each quarter turn (larger at the start, smaller later)
        let plat_x = ramp_radius * cos_b;
        let plat_z = ramp_radius * sin_b;
        let plat_y = base_y + rise_per_segment;
        let plat_size = if i < 6 { 12.0 } else { 8.0 };

        spawn_cube(
            world,
            &format!("Rest Platform {}", i + 1),
            Transform::from_translation(Vec3::new(plat_x, plat_y, plat_z))
                .with_scale(Vec3::new(plat_size, 0.5, plat_size)),
            stone.0,
            stone.1,
            [3.0, 3.0],
            Color::srgb(0.65, 0.65, 0.7),
        );

        // Rails on early ramp segments (removed for later difficulty)
        if i < 8 {
            let rail_offset = width * 0.55;
            let perp = Vec3::new(-dir.z, 0.0, dir.x);
            for (sign, rname) in [(-1.0, "L"), (1.0, "R")] {
                let rail_pos = Vec3::new(cx, mid_y + 0.5, cz) + perp * sign * rail_offset;
                spawn_cube(
                    world,
                    &format!("Ramp {} Rail {}", i + 1, rname),
                    Transform::from_translation(rail_pos)
                        .with_rotation(
                            Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-slope_angle),
                        )
                        .with_scale(Vec3::new(0.2, 1.0, ramp_len)),
                    metal.0,
                    metal.1,
                    [1.0, 2.0],
                    Color::srgb(0.5, 0.5, 0.55),
                );
            }
        }
    }

    // =========================================================================
    // Top Platform (Goal)
    // =========================================================================

    let top_y = start_y + num_segments as f32 * rise_per_segment;

    spawn_cube(
        world,
        "Top Platform",
        Transform::from_translation(Vec3::new(0.0, top_y + 0.25, 0.0))
            .with_scale(Vec3::new(20.0, 0.5, 20.0)),
        stone.0,
        stone.1,
        [4.0, 4.0],
        Color::srgb(0.85, 0.8, 0.75),
    );

    // Goal zone
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
        Transform::from_translation(Vec3::new(0.0, top_y + 2.0, 0.0))
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
            illuminance: 18000.0,
            shadows_enabled: true,
        },
        Transform::from_translation(Vec3::new(30.0, 100.0, 20.0))
            .looking_at(Vec3::new(0.0, 40.0, 0.0), Vec3::Y),
    ));

    info!("Built The Spiral level");
}
