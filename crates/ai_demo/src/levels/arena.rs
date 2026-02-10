//! Arena level: open area with obstacles, spawn points, and waypoints.

use super::level_gen::{spawn_cube, spawn_cylinder};
use bevy::prelude::*;

use crate::{SpawnPoint, Waypoint};

/// Build the arena level — serializable components only.
pub fn build_arena(world: &mut World) {
    let stone = ("textures/stone_albedo.png", "textures/stone_normal.png");
    let metal = ("textures/metal_albedo.png", "textures/metal_normal.png");

    // =========================================================================
    // Ground
    // =========================================================================

    spawn_cube(
        world,
        "Ground",
        Transform::from_translation(Vec3::new(0.0, -0.5, 0.0))
            .with_scale(Vec3::new(30.0, 1.0, 30.0)),
        stone.0,
        stone.1,
        [6.0, 6.0],
        Color::srgb(0.85, 0.82, 0.78),
    );

    // =========================================================================
    // Perimeter Walls
    // =========================================================================

    let wall_color = Color::srgb(0.55, 0.55, 0.6);
    for (pos, scale, name) in [
        (
            Vec3::new(-15.0, 1.5, 0.0),
            Vec3::new(0.5, 3.5, 30.0),
            "Wall West",
        ),
        (
            Vec3::new(15.0, 1.5, 0.0),
            Vec3::new(0.5, 3.5, 30.0),
            "Wall East",
        ),
        (
            Vec3::new(0.0, 1.5, -15.0),
            Vec3::new(30.0, 3.5, 0.5),
            "Wall North",
        ),
        (
            Vec3::new(0.0, 1.5, 15.0),
            Vec3::new(30.0, 3.5, 0.5),
            "Wall South",
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
    // Central Obstacles
    // =========================================================================

    // Large central block
    spawn_cube(
        world,
        "Central Block",
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0))
            .with_scale(Vec3::new(4.0, 2.0, 4.0)),
        stone.0,
        stone.1,
        [2.0, 2.0],
        Color::srgb(0.6, 0.58, 0.55),
    );

    // L-shaped barrier (two cubes)
    spawn_cube(
        world,
        "L-Barrier Long",
        Transform::from_translation(Vec3::new(-6.0, 0.75, -5.0))
            .with_scale(Vec3::new(8.0, 1.5, 1.0)),
        metal.0,
        metal.1,
        [4.0, 1.0],
        Color::srgb(0.5, 0.5, 0.55),
    );

    spawn_cube(
        world,
        "L-Barrier Short",
        Transform::from_translation(Vec3::new(-2.5, 0.75, -8.0))
            .with_scale(Vec3::new(1.0, 1.5, 5.0)),
        metal.0,
        metal.1,
        [1.0, 2.0],
        Color::srgb(0.5, 0.5, 0.55),
    );

    // Scattered pillars
    let pillar_color = Color::srgb(0.45, 0.45, 0.5);
    for (pos, name) in [
        (Vec3::new(6.0, 1.5, 5.0), "Pillar NE"),
        (Vec3::new(8.0, 1.5, -3.0), "Pillar E"),
        (Vec3::new(-8.0, 1.5, 6.0), "Pillar NW"),
        (Vec3::new(4.0, 1.5, -8.0), "Pillar SE"),
    ] {
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

    // Corridor walls creating a narrow passage on the east side
    spawn_cube(
        world,
        "Passage Wall A",
        Transform::from_translation(Vec3::new(10.0, 0.75, 3.0))
            .with_scale(Vec3::new(0.5, 1.5, 10.0)),
        metal.0,
        metal.1,
        [1.0, 3.0],
        Color::srgb(0.5, 0.5, 0.55),
    );

    spawn_cube(
        world,
        "Passage Wall B",
        Transform::from_translation(Vec3::new(12.5, 0.75, 3.0))
            .with_scale(Vec3::new(0.5, 1.5, 10.0)),
        metal.0,
        metal.1,
        [1.0, 3.0],
        Color::srgb(0.5, 0.5, 0.55),
    );

    // Low cover blocks
    spawn_cube(
        world,
        "Cover Block A",
        Transform::from_translation(Vec3::new(-4.0, 0.5, 7.0))
            .with_scale(Vec3::new(3.0, 1.0, 1.5)),
        stone.0,
        stone.1,
        [2.0, 1.0],
        Color::srgb(0.65, 0.6, 0.55),
    );

    spawn_cube(
        world,
        "Cover Block B",
        Transform::from_translation(Vec3::new(5.0, 0.5, -11.0))
            .with_scale(Vec3::new(2.0, 1.0, 2.0)),
        stone.0,
        stone.1,
        [1.0, 1.0],
        Color::srgb(0.65, 0.6, 0.55),
    );

    // =========================================================================
    // Spawn Points (where agents start)
    // =========================================================================

    world.spawn((
        bevy_modal_editor::SceneEntity,
        SpawnPoint,
        Name::new("Spawn Point A"),
        Transform::from_translation(Vec3::new(-10.0, 0.5, 10.0)),
    ));

    world.spawn((
        bevy_modal_editor::SceneEntity,
        SpawnPoint,
        Name::new("Spawn Point B"),
        Transform::from_translation(Vec3::new(-10.0, 0.5, -10.0)),
    ));

    // =========================================================================
    // Waypoints (agent targets)
    // =========================================================================

    world.spawn((
        bevy_modal_editor::SceneEntity,
        Waypoint,
        Name::new("Waypoint A"),
        Transform::from_translation(Vec3::new(10.0, 0.5, -10.0)),
    ));

    world.spawn((
        bevy_modal_editor::SceneEntity,
        Waypoint,
        Name::new("Waypoint B"),
        Transform::from_translation(Vec3::new(10.0, 0.5, 10.0)),
    ));

    world.spawn((
        bevy_modal_editor::SceneEntity,
        Waypoint,
        Name::new("Waypoint C"),
        Transform::from_translation(Vec3::new(0.0, 0.5, -12.0)),
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

    info!("Built arena level");
}
