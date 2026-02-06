//! Locomotion Arena — a large interconnected arena testing all locomotion features.
//!
//! Layout (roughly circular, clockwise from spawn):
//! - Starting Courtyard (center) — walking/sprinting
//! - Jump Pillars (north) — jumping precision
//! - Step-Up Corridor (NE) — auto step-up
//! - Ledge Wall (east) — ledge grab + climb
//! - Wall-Jump Canyon (SE) — wall-jump chains
//! - Sprint-Slide Tunnel (south) — sprint → slide
//! - Slide-Jump Ramp (SW) — slide-jump boost
//! - Ladder Tower (west) — ladder climbing
//! - Steep Slopes (NW) — forced slope sliding
//! - Crouch Maze (surface, NE) — crouching through low-ceiling passages

use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_editor_game::MaterialRef;
use bevy_modal_editor::SceneEntity;

use crate::{Coin, LadderSurface, LedgeWall, SpawnPoint, SteepSlope};

use super::level_gen::{spawn_cube, spawn_cylinder};

/// Texture shorthand
fn stone() -> (&'static str, &'static str) {
    ("textures/stone_albedo.png", "textures/stone_normal.png")
}

fn metal() -> (&'static str, &'static str) {
    ("textures/metal_albedo.png", "textures/metal_normal.png")
}

fn wood() -> (&'static str, &'static str) {
    ("textures/wood_albedo.png", "textures/wood_normal.png")
}

fn brick() -> (&'static str, &'static str) {
    ("textures/brick_albedo.png", "textures/brick_normal.png")
}

/// Spawn a coin at a position.
fn spawn_coin(world: &mut World, name: &str, pos: Vec3) {
    world.spawn((
        SceneEntity,
        Coin,
        Name::new(name.to_string()),
        Transform::from_translation(pos),
    ));
}

/// Spawn a cube with a locomotion marker component.
fn spawn_marked_cube<C: Component>(
    world: &mut World,
    name: &str,
    transform: Transform,
    albedo: &str,
    normal: &str,
    uv_scale: [f32; 2],
    base_color: Color,
    marker: C,
) {
    world.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        bevy_modal_editor::PrimitiveMarker {
            shape: bevy_modal_editor::PrimitiveShape::Cube,
        },
        MaterialRef::Inline(bevy_editor_game::MaterialDefinition {
            base: bevy_editor_game::BaseMaterialProps {
                base_color,
                base_color_texture: Some(albedo.into()),
                normal_map_texture: Some(normal.into()),
                uv_scale,
                ..default()
            },
            extension: None,
        }),
        transform,
        RigidBody::Static,
        marker,
    ));
}

pub fn build_locomotion_arena(world: &mut World) {
    let stone = stone();
    let metal = metal();
    let wood = wood();
    let brick = brick();

    // =========================================================================
    // Ground plane: 80x1x80 at Y=-0.5
    // =========================================================================
    spawn_cube(
        world,
        "Ground",
        Transform::from_translation(Vec3::new(0.0, -0.5, 0.0))
            .with_scale(Vec3::new(80.0, 1.0, 80.0)),
        stone.0,
        stone.1,
        [16.0, 16.0],
        Color::srgb(0.6, 0.6, 0.6),
    );

    // =========================================================================
    // Perimeter walls (6m tall, 1m thick)
    // =========================================================================
    // North wall
    spawn_cube(
        world,
        "Wall North",
        Transform::from_translation(Vec3::new(0.0, 3.0, -40.0))
            .with_scale(Vec3::new(80.0, 6.0, 1.0)),
        brick.0,
        brick.1,
        [16.0, 2.0],
        Color::srgb(0.5, 0.4, 0.35),
    );
    // South wall
    spawn_cube(
        world,
        "Wall South",
        Transform::from_translation(Vec3::new(0.0, 3.0, 40.0))
            .with_scale(Vec3::new(80.0, 6.0, 1.0)),
        brick.0,
        brick.1,
        [16.0, 2.0],
        Color::srgb(0.5, 0.4, 0.35),
    );
    // East wall
    spawn_cube(
        world,
        "Wall East",
        Transform::from_translation(Vec3::new(40.0, 3.0, 0.0))
            .with_scale(Vec3::new(1.0, 6.0, 80.0)),
        brick.0,
        brick.1,
        [16.0, 2.0],
        Color::srgb(0.5, 0.4, 0.35),
    );
    // West wall
    spawn_cube(
        world,
        "Wall West",
        Transform::from_translation(Vec3::new(-40.0, 3.0, 0.0))
            .with_scale(Vec3::new(1.0, 6.0, 80.0)),
        brick.0,
        brick.1,
        [16.0, 2.0],
        Color::srgb(0.5, 0.4, 0.35),
    );

    // =========================================================================
    // Spawn Point (center courtyard)
    // =========================================================================
    world.spawn((
        SceneEntity,
        SpawnPoint,
        Name::new("Spawn Point"),
        Transform::from_translation(Vec3::new(0.0, 1.0, 0.0)),
    ));

    // =========================================================================
    // Lighting
    // =========================================================================
    world.spawn((
        SceneEntity,
        Name::new("Sun"),
        bevy_modal_editor::DirectionalLightMarker {
            color: Color::WHITE,
            illuminance: 15000.0,
            shadows_enabled: true,
        },
        Transform::from_translation(Vec3::new(20.0, 40.0, 20.0))
            .looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // =========================================================================
    // Section 1: Jump Pillars (North, +Z direction from center → -Z in world)
    // Located at Z = -15 to -30
    // =========================================================================
    // Series of pillars at increasing heights
    let pillar_positions = [
        (Vec3::new(0.0, 0.5, -15.0), Vec3::new(2.0, 1.0, 2.0)),
        (Vec3::new(3.0, 1.0, -18.0), Vec3::new(2.0, 2.0, 2.0)),
        (Vec3::new(0.0, 1.5, -21.0), Vec3::new(2.0, 3.0, 2.0)),
        (Vec3::new(-3.0, 2.0, -24.0), Vec3::new(2.0, 4.0, 2.0)),
        (Vec3::new(0.0, 3.0, -27.0), Vec3::new(2.0, 6.0, 2.0)),
    ];
    for (i, (pos, scale)) in pillar_positions.iter().enumerate() {
        spawn_cube(
            world,
            &format!("Jump Pillar {}", i + 1),
            Transform::from_translation(*pos).with_scale(*scale),
            stone.0,
            stone.1,
            [1.0, 1.0],
            Color::srgb(0.7, 0.7, 0.75),
        );
    }
    // Coin 1: top of tallest pillar
    spawn_coin(world, "Coin 1 - Jump Pillar Top", Vec3::new(0.0, 7.0, -27.0));

    // =========================================================================
    // Section 2: Step-Up Corridor (NE, X=15..25, Z=-15..-25)
    // =========================================================================
    // A series of low walls that the player auto-steps over
    for i in 0..6 {
        let z = -15.0 - (i as f32) * 2.0;
        let height = 0.3; // Just under step-up threshold (0.35m)
        spawn_cube(
            world,
            &format!("Step Wall {}", i + 1),
            Transform::from_translation(Vec3::new(20.0, height * 0.5, z))
                .with_scale(Vec3::new(6.0, height, 0.5)),
            metal.0,
            metal.1,
            [2.0, 1.0],
            Color::srgb(0.5, 0.55, 0.6),
        );
    }
    // Side walls for corridor
    spawn_cube(
        world,
        "Step Corridor Wall L",
        Transform::from_translation(Vec3::new(17.0, 1.5, -20.0))
            .with_scale(Vec3::new(0.5, 3.0, 14.0)),
        brick.0,
        brick.1,
        [4.0, 1.0],
        Color::srgb(0.5, 0.4, 0.35),
    );
    spawn_cube(
        world,
        "Step Corridor Wall R",
        Transform::from_translation(Vec3::new(23.0, 1.5, -20.0))
            .with_scale(Vec3::new(0.5, 3.0, 14.0)),
        brick.0,
        brick.1,
        [4.0, 1.0],
        Color::srgb(0.5, 0.4, 0.35),
    );
    // Coin 2: end of step-up corridor
    spawn_coin(world, "Coin 2 - Step Corridor End", Vec3::new(20.0, 1.5, -26.0));

    // =========================================================================
    // Section 3: Ledge Wall (East, X=25..35, Z=-5..5)
    // =========================================================================
    // Tall wall with LedgeGrabbable marker
    spawn_marked_cube(
        world,
        "Ledge Wall Main",
        Transform::from_translation(Vec3::new(30.0, 3.0, 0.0))
            .with_scale(Vec3::new(1.0, 6.0, 10.0)),
        brick.0,
        brick.1,
        [3.0, 2.0],
        Color::srgb(0.6, 0.45, 0.35),
        LedgeWall,
    );
    // Platform on top of ledge wall
    spawn_cube(
        world,
        "Ledge Top Platform",
        Transform::from_translation(Vec3::new(32.0, 6.0, 0.0))
            .with_scale(Vec3::new(4.0, 0.5, 10.0)),
        stone.0,
        stone.1,
        [2.0, 2.0],
        Color::srgb(0.65, 0.65, 0.7),
    );
    // Coin 3: on the ledge top platform
    spawn_coin(world, "Coin 3 - Ledge Top", Vec3::new(32.0, 7.0, 0.0));

    // =========================================================================
    // Section 4: Wall-Jump Canyon (SE, X=20..30, Z=10..25)
    // =========================================================================
    // Two parallel walls close enough for wall-jumping
    spawn_marked_cube(
        world,
        "Canyon Wall Left",
        Transform::from_translation(Vec3::new(22.0, 4.0, 18.0))
            .with_scale(Vec3::new(1.0, 8.0, 12.0)),
        stone.0,
        stone.1,
        [4.0, 2.0],
        Color::srgb(0.55, 0.55, 0.6),
        LedgeWall,
    );
    spawn_marked_cube(
        world,
        "Canyon Wall Right",
        Transform::from_translation(Vec3::new(26.0, 4.0, 18.0))
            .with_scale(Vec3::new(1.0, 8.0, 12.0)),
        stone.0,
        stone.1,
        [4.0, 2.0],
        Color::srgb(0.55, 0.55, 0.6),
        LedgeWall,
    );
    // Coin 4: high up, only reachable by wall-jump chain
    spawn_coin(world, "Coin 4 - Wall Jump Canyon", Vec3::new(24.0, 7.0, 18.0));

    // =========================================================================
    // Section 5: Sprint-Slide Tunnel (South, Z=15..30)
    // =========================================================================
    // Long corridor with low ceiling section requiring slide
    // Raised floor so it sits on ground (top at Y=0.25)
    spawn_cube(
        world,
        "Slide Tunnel Floor",
        Transform::from_translation(Vec3::new(0.0, 0.125, 25.0))
            .with_scale(Vec3::new(6.0, 0.25, 16.0)),
        metal.0,
        metal.1,
        [2.0, 4.0],
        Color::srgb(0.5, 0.55, 0.6),
    );
    // Side walls
    spawn_cube(
        world,
        "Slide Tunnel Wall L",
        Transform::from_translation(Vec3::new(-3.0, 1.5, 25.0))
            .with_scale(Vec3::new(0.5, 3.0, 16.0)),
        brick.0,
        brick.1,
        [4.0, 1.0],
        Color::srgb(0.5, 0.4, 0.35),
    );
    spawn_cube(
        world,
        "Slide Tunnel Wall R",
        Transform::from_translation(Vec3::new(3.0, 1.5, 25.0))
            .with_scale(Vec3::new(0.5, 3.0, 16.0)),
        brick.0,
        brick.1,
        [4.0, 1.0],
        Color::srgb(0.5, 0.4, 0.35),
    );
    // Low ceiling (1.0m clearance — must slide under, standing height is 1.8m)
    spawn_cube(
        world,
        "Slide Tunnel Ceiling",
        Transform::from_translation(Vec3::new(0.0, 1.25, 28.0))
            .with_scale(Vec3::new(6.0, 0.3, 6.0)),
        metal.0,
        metal.1,
        [2.0, 2.0],
        Color::srgb(0.45, 0.5, 0.55),
    );
    // Coin 5: inside the low-ceiling passage
    spawn_coin(world, "Coin 5 - Slide Tunnel", Vec3::new(0.0, 1.0, 28.0));

    // =========================================================================
    // Section 6: Slide-Jump Ramp (SW, X=-15..-25, Z=10..20)
    // =========================================================================
    // Ramp that provides slide-jump boost to reach high platform
    spawn_cube(
        world,
        "Slide Ramp",
        Transform::from_translation(Vec3::new(-20.0, 1.5, 15.0))
            .with_scale(Vec3::new(6.0, 0.5, 10.0))
            .with_rotation(Quat::from_rotation_x(-0.2)),
        metal.0,
        metal.1,
        [2.0, 2.0],
        Color::srgb(0.5, 0.55, 0.6),
    );
    // High floating platform (reachable only with slide-jump boost)
    spawn_cube(
        world,
        "Slide Jump Target",
        Transform::from_translation(Vec3::new(-20.0, 4.5, 22.0))
            .with_scale(Vec3::new(4.0, 0.5, 4.0)),
        wood.0,
        wood.1,
        [2.0, 2.0],
        Color::srgb(0.7, 0.55, 0.35),
    );
    // Coin 6: on the high floating platform
    spawn_coin(world, "Coin 6 - Slide Jump Platform", Vec3::new(-20.0, 5.5, 22.0));

    // =========================================================================
    // Section 7: Ladder Tower (West, X=-25..-35, Z=-5..5)
    // =========================================================================
    // Tower structure with ladder surface
    // Tower base
    spawn_cube(
        world,
        "Ladder Tower Base",
        Transform::from_translation(Vec3::new(-30.0, 2.5, 0.0))
            .with_scale(Vec3::new(6.0, 5.0, 6.0)),
        stone.0,
        stone.1,
        [2.0, 2.0],
        Color::srgb(0.6, 0.6, 0.65),
    );
    // Ladder surface (east face of tower)
    spawn_marked_cube(
        world,
        "Ladder Surface Lower",
        Transform::from_translation(Vec3::new(-27.0, 2.5, 0.0))
            .with_scale(Vec3::new(0.3, 5.0, 2.0)),
        wood.0,
        wood.1,
        [1.0, 2.0],
        Color::srgb(0.6, 0.45, 0.25),
        LadderSurface,
    );
    // Middle platform
    spawn_cube(
        world,
        "Ladder Mid Platform",
        Transform::from_translation(Vec3::new(-28.0, 5.0, 0.0))
            .with_scale(Vec3::new(4.0, 0.5, 6.0)),
        stone.0,
        stone.1,
        [2.0, 2.0],
        Color::srgb(0.6, 0.6, 0.65),
    );
    // Coin 7: midway up on the platform
    spawn_coin(world, "Coin 7 - Ladder Mid", Vec3::new(-28.0, 6.0, 0.0));

    // Upper tower section
    spawn_cube(
        world,
        "Ladder Tower Upper",
        Transform::from_translation(Vec3::new(-30.0, 7.5, 0.0))
            .with_scale(Vec3::new(6.0, 5.0, 6.0)),
        stone.0,
        stone.1,
        [2.0, 2.0],
        Color::srgb(0.6, 0.6, 0.65),
    );
    // Upper ladder surface
    spawn_marked_cube(
        world,
        "Ladder Surface Upper",
        Transform::from_translation(Vec3::new(-27.0, 7.5, 0.0))
            .with_scale(Vec3::new(0.3, 5.0, 2.0)),
        wood.0,
        wood.1,
        [1.0, 2.0],
        Color::srgb(0.6, 0.45, 0.25),
        LadderSurface,
    );
    // Top platform
    spawn_cube(
        world,
        "Ladder Top Platform",
        Transform::from_translation(Vec3::new(-30.0, 10.0, 0.0))
            .with_scale(Vec3::new(8.0, 0.5, 8.0)),
        metal.0,
        metal.1,
        [2.0, 2.0],
        Color::srgb(0.55, 0.55, 0.6),
    );
    // Coin 8: at the very top
    spawn_coin(world, "Coin 8 - Ladder Top", Vec3::new(-30.0, 11.0, 0.0));

    // =========================================================================
    // Section 8: Steep Slopes (NW, X=-15..-25, Z=-15..-25)
    // =========================================================================
    // Steep angled surfaces with ForceSlide marker
    spawn_marked_cube(
        world,
        "Steep Slope 1",
        Transform::from_translation(Vec3::new(-20.0, 2.0, -20.0))
            .with_scale(Vec3::new(8.0, 0.5, 8.0))
            .with_rotation(Quat::from_rotation_x(-0.8)), // ~46 degrees
        stone.0,
        stone.1,
        [2.0, 2.0],
        Color::srgb(0.5, 0.6, 0.5),
        SteepSlope,
    );
    // Shelf midway on the slope
    spawn_cube(
        world,
        "Slope Shelf",
        Transform::from_translation(Vec3::new(-20.0, 2.5, -18.0))
            .with_scale(Vec3::new(3.0, 0.5, 2.0)),
        stone.0,
        stone.1,
        [1.0, 1.0],
        Color::srgb(0.65, 0.65, 0.7),
    );
    // Coin 9: on the shelf mid-slope
    spawn_coin(world, "Coin 9 - Slope Shelf", Vec3::new(-20.0, 3.5, -18.0));

    // =========================================================================
    // Section 9: Crouch Maze (surface level, NE area, X=8..22, Z=2..16)
    // Low-ceiling structure on the ground — must crouch (1.0m) to navigate.
    // Ceiling at Y=1.1 gives just enough clearance for crouched player.
    // =========================================================================
    // Maze floor (slightly raised to distinguish from ground)
    spawn_cube(
        world,
        "Maze Floor",
        Transform::from_translation(Vec3::new(12.0, 0.05, 9.0))
            .with_scale(Vec3::new(14.0, 0.1, 14.0)),
        stone.0,
        stone.1,
        [4.0, 4.0],
        Color::srgb(0.35, 0.35, 0.4),
    );
    // Low ceiling — 1.1m above ground, crouched player is 1.0m
    spawn_cube(
        world,
        "Maze Ceiling",
        Transform::from_translation(Vec3::new(12.0, 1.2, 9.0))
            .with_scale(Vec3::new(14.0, 0.2, 14.0)),
        stone.0,
        stone.1,
        [4.0, 4.0],
        Color::srgb(0.3, 0.3, 0.35),
    );
    // Outer walls (enclose the maze, open entrance on west side)
    // North wall
    spawn_cube(
        world,
        "Maze Outer N",
        Transform::from_translation(Vec3::new(12.0, 0.6, 2.0))
            .with_scale(Vec3::new(14.0, 1.2, 0.5)),
        stone.0,
        stone.1,
        [4.0, 1.0],
        Color::srgb(0.4, 0.4, 0.45),
    );
    // South wall
    spawn_cube(
        world,
        "Maze Outer S",
        Transform::from_translation(Vec3::new(12.0, 0.6, 16.0))
            .with_scale(Vec3::new(14.0, 1.2, 0.5)),
        stone.0,
        stone.1,
        [4.0, 1.0],
        Color::srgb(0.4, 0.4, 0.45),
    );
    // East wall
    spawn_cube(
        world,
        "Maze Outer E",
        Transform::from_translation(Vec3::new(19.0, 0.6, 9.0))
            .with_scale(Vec3::new(0.5, 1.2, 14.0)),
        stone.0,
        stone.1,
        [4.0, 1.0],
        Color::srgb(0.4, 0.4, 0.45),
    );
    // West wall with entrance gap (gap from Z=7 to Z=11)
    spawn_cube(
        world,
        "Maze Outer W1",
        Transform::from_translation(Vec3::new(5.0, 0.6, 4.0))
            .with_scale(Vec3::new(0.5, 1.2, 4.0)),
        stone.0,
        stone.1,
        [2.0, 1.0],
        Color::srgb(0.4, 0.4, 0.45),
    );
    spawn_cube(
        world,
        "Maze Outer W2",
        Transform::from_translation(Vec3::new(5.0, 0.6, 14.0))
            .with_scale(Vec3::new(0.5, 1.2, 4.0)),
        stone.0,
        stone.1,
        [2.0, 1.0],
        Color::srgb(0.4, 0.4, 0.45),
    );

    // Internal maze partitions
    let maze_walls = [
        (Vec3::new(8.0, 0.6, 5.0), Vec3::new(0.4, 1.2, 6.0)),
        (Vec3::new(12.0, 0.6, 7.0), Vec3::new(6.0, 1.2, 0.4)),
        (Vec3::new(16.0, 0.6, 11.0), Vec3::new(0.4, 1.2, 6.0)),
        (Vec3::new(10.0, 0.6, 13.0), Vec3::new(6.0, 1.2, 0.4)),
    ];
    for (i, (pos, scale)) in maze_walls.iter().enumerate() {
        spawn_cube(
            world,
            &format!("Maze Wall {}", i + 1),
            Transform::from_translation(*pos).with_scale(*scale),
            stone.0,
            stone.1,
            [1.0, 1.0],
            Color::srgb(0.4, 0.4, 0.45),
        );
    }
    // Coin 10: deep in the maze (back-right corner)
    spawn_coin(world, "Coin 10 - Crouch Maze", Vec3::new(17.0, 0.8, 14.0));

    // =========================================================================
    // Connecting pathways / decorative elements
    // =========================================================================
    // Small guide pillars around the courtyard pointing to sections
    let guide_positions = [
        Vec3::new(0.0, 0.75, -8.0),   // North → Jump Pillars
        Vec3::new(12.0, 0.75, -12.0),  // NE → Step-Up
        Vec3::new(20.0, 0.75, 0.0),    // East → Ledge
        Vec3::new(15.0, 0.75, 12.0),   // SE → Canyon
        Vec3::new(0.0, 0.75, 12.0),    // South → Slide
        Vec3::new(-12.0, 0.75, 12.0),  // SW → Ramp
        Vec3::new(-20.0, 0.75, 0.0),   // West → Ladder
        Vec3::new(-12.0, 0.75, -12.0), // NW → Slopes
    ];
    for (i, pos) in guide_positions.iter().enumerate() {
        spawn_cylinder(
            world,
            &format!("Guide Post {}", i + 1),
            Transform::from_translation(*pos).with_scale(Vec3::new(0.3, 1.5, 0.3)),
            metal.0,
            metal.1,
            [1.0, 1.0],
            Color::srgb(0.7, 0.7, 0.75),
        );
    }

    info!("Built Locomotion Arena level");
}
