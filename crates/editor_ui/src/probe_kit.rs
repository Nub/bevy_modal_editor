//! Kit/socket flow probe (KIT_PROBE=1): drives the D9/D10 surface with real
//! input — place a wall, `o`-chain it, drag + commit (snap-back), then PAINT a
//! polyline with corner resolution — asserting mated geometry and on-screen
//! outcomes. Runs in `verify.sh full` and CI beside the other probes.

use crate::probe_user::{click, move_cursor, shot, tap, tap_named, viewport_center};
use bevy::input::keyboard::Key;
use bevy::prelude::*;
use editor_prefabs::sockets::Socket;
use editor_prefabs::{PrefabInstance, PrefabLibrary, paint::PaintState};
use editor_scene::PrefabStamped;

#[derive(Resource, Default)]
pub(crate) struct KitProbe {
    frame: u32,
    pub(crate) failures: Vec<String>,
    walls_before_paint: usize,
}

fn instance_roots_named(world: &mut World, name: &str) -> Vec<(Entity, Vec3)> {
    world
        .query_filtered::<(Entity, &Name, &Transform), (With<PrefabInstance>, Without<PrefabStamped>)>()
        .iter(world)
        .filter(|(_, n, _)| n.as_str() == name)
        .map(|(e, _, t)| (e, t.translation))
        .collect()
}

/// Two instances are MATED when any socket of one coincides with any socket
/// of the other (within 5cm).
fn mated(world: &mut World, a: Entity, b: Entity) -> bool {
    let sockets_of = |world: &mut World, root: Entity| -> Vec<Vec3> {
        let members: Vec<Entity> = {
            let mut stack = vec![root];
            let mut all = vec![];
            while let Some(current) = stack.pop() {
                all.push(current);
                if let Some(children) = world.get::<Children>(current) {
                    stack.extend(children.iter());
                }
            }
            all
        };
        members
            .into_iter()
            .filter(|e| world.get::<Socket>(*e).is_some())
            .filter_map(|e| world.get::<GlobalTransform>(e).map(|g| g.translation()))
            .collect()
    };
    let sa = sockets_of(world, a);
    let sb = sockets_of(world, b);
    sa.iter().any(|p| sb.iter().any(|q| p.distance(*q) < 0.05))
}

fn check(world: &mut World, ok: bool, what: &str) {
    if ok {
        info!("KIT-PROBE PASS: {what}");
    } else {
        error!("KIT-PROBE FAIL: {what}");
        world
            .resource_mut::<KitProbe>()
            .failures
            .push(what.to_string());
    }
}

pub(crate) fn probe_kit(world: &mut World) {
    world.resource_mut::<KitProbe>().frame += 1;
    let frame = world.resource::<KitProbe>().frame;
    let fail =
        |world: &mut World, what: &str| world.resource_mut::<KitProbe>().failures.push(what.into());
    // Clicks live OUTSIDE the match: the cursor-assert ranges below would
    // otherwise shadow these frames (match arms are first-win).
    match frame {
        770 | 810 | 890 => click(world, true),
        772 | 812 | 892 => click(world, false),
        _ => {}
    }
    match frame {
        60 => tap_named(world, KeyCode::Enter, Key::Enter),
        120 => tap_named(world, KeyCode::F12, Key::F12),
        180 => {
            let has_kit = world
                .resource::<PrefabLibrary>()
                .prefabs
                .values()
                .any(|d| d.name == "Wall" && d.kit.is_some());
            check(world, has_kit, "demo Wall loads with a kit tag");
        }
        // ── Place a wall via the palette ───────────────────────────────────
        200 => tap(world, KeyCode::KeyI, "i"),
        240 => {
            for (code, ch) in [
                (KeyCode::KeyW, "w"),
                (KeyCode::KeyA, "a"),
                (KeyCode::KeyL, "l"),
            ] {
                tap(world, code, ch);
            }
        }
        280 => tap_named(world, KeyCode::Enter, Key::Enter),
        340 => {
            let walls = instance_roots_named(world, "Wall");
            check(world, walls.len() == 1, "one Wall placed from the palette");
        }
        // ── o chains a second wall, mated ──────────────────────────────────
        360 => tap(world, KeyCode::KeyO, "o"),
        420 => {
            let walls = instance_roots_named(world, "Wall");
            if walls.len() != 2 {
                fail(world, "o chained a second Wall");
                return;
            }
            let is_mated = mated(world, walls[0].0, walls[1].0);
            check(
                world,
                is_mated,
                "o-chained wall is socket-mated to the first",
            );
            let spacing = walls[0].1.distance(walls[1].1);
            check(
                world,
                (spacing - 2.0).abs() < 0.05,
                "chained wall exactly one length away",
            );
            shot(world, "k1-chained");
        }
        // ── Drag the selected wall a little; commit snaps it back mated ───
        440 => tap(world, KeyCode::KeyW, "w"),
        450..=470 if frame.is_multiple_of(2) => {
            let current = crate::probe_user::window_of(world)
                .and_then(|w| world.get::<Window>(w))
                .and_then(|w| w.cursor_position())
                .unwrap_or(viewport_center(world));
            move_cursor(world, current + Vec2::new(8.0, 0.0));
        }
        480 => tap_named(world, KeyCode::Enter, Key::Enter),
        560 => {
            let walls = instance_roots_named(world, "Wall");
            if walls.len() == 2 {
                let is_mated = mated(world, walls[0].0, walls[1].0);
                check(
                    world,
                    is_mated,
                    "move-commit snapped the wall back into its mate",
                );
            } else {
                fail(world, "move-commit kept two walls");
            }
            shot(world, "k2-snap-after-move");
        }
        // ── Paint a polyline with a corner ─────────────────────────────────
        600 => {
            let center = viewport_center(world);
            move_cursor(world, center);
        }
        620 => {
            let shift = Key::Shift;
            crate::probe_user::key(world, KeyCode::ShiftLeft, shift, None, true);
        }
        624 => tap(world, KeyCode::Semicolon, ":"),
        630 => crate::probe_user::key(world, KeyCode::ShiftLeft, Key::Shift, None, false),
        660 => {
            for (code, ch) in [
                (KeyCode::KeyP, "p"),
                (KeyCode::KeyA, "a"),
                (KeyCode::KeyI, "i"),
                (KeyCode::KeyN, "n"),
                (KeyCode::KeyT, "t"),
            ] {
                tap(world, code, ch);
            }
        }
        700 => tap_named(world, KeyCode::Enter, Key::Enter),
        740 => {
            let painting = world.resource::<PaintState>().0.is_some();
            check(world, painting, "Paint With Piece arms the paint overlay");
        }
        // Anchor left, lay a long run right, then turn toward the camera.
        750 => {
            let count = instance_roots_named(world, "Wall").len();
            world.resource_mut::<KitProbe>().walls_before_paint = count;
        }
        // Re-assert the cursor EVERY frame around clicks: winit refreshes the
        // window's cursor from the real device, clobbering one-shot injection.
        758..=772 => {
            let size = viewport_center(world) * 2.0;
            move_cursor(world, Vec2::new(size.x * 0.25, size.y * 0.70));
        }
        798..=812 => {
            let size = viewport_center(world) * 2.0;
            move_cursor(world, Vec2::new(size.x * 0.78, size.y * 0.70));
        }
        860 => {
            let before = world.resource::<KitProbe>().walls_before_paint;
            let walls = instance_roots_named(world, "Wall").len();
            check(world, walls > before, "paint stroke laid a run of walls");
        }
        878..=892 => {
            let size = viewport_center(world) * 2.0;
            move_cursor(world, Vec2::new(size.x * 0.78, size.y * 0.92));
        }
        940 => {
            let corners = instance_roots_named(world, "Corner").len();
            check(
                world,
                corners >= 1,
                "direction change inserted the kit's Corner",
            );
            shot(world, "k3-painted-corner");
        }
        960 => tap_named(world, KeyCode::Escape, Key::Escape),
        1000 => {
            let done = world.resource::<PaintState>().0.is_none();
            check(world, done, "Esc leaves paint mode");
        }
        1060 => {
            let failures = world.resource::<KitProbe>().failures.clone();
            if failures.is_empty() {
                info!("KIT-PROBE PASS: sockets, chaining, snap, painting");
                world.write_message(bevy::app::AppExit::Success);
            } else {
                for f in &failures {
                    error!("KIT-PROBE FAIL: {f}");
                }
                world.write_message(bevy::app::AppExit::error());
            }
        }
        _ => {}
    }
}
