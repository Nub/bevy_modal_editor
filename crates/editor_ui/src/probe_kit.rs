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
    socket_at: Option<Vec3>,
    joint_at: Option<Vec3>,
    far_at: Option<Vec3>,
}

/// A socket with nothing joined to it — the end of a run, which is where a
/// designer builds from.
fn free_socket(world: &mut World) -> Option<(Entity, Vec3)> {
    let sockets: Vec<(Entity, GlobalTransform)> = world
        .query::<(Entity, &GlobalTransform, &Socket)>()
        .iter(world)
        .map(|(entity, global, _)| (entity, *global))
        .collect();
    sockets
        .iter()
        .find(|(entity, global)| {
            !sockets.iter().any(|(other, other_global)| {
                other != entity && editor_prefabs::sockets::sockets_are_joined(global, other_global)
            })
        })
        .map(|(entity, global)| (*entity, global.translation()))
}

/// Every socket under a piece, in world space.
fn sockets_under(world: &mut World, root: Entity) -> Vec<(Entity, GlobalTransform)> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if world.get::<Socket>(entity).is_some()
            && let Some(global) = world.get::<GlobalTransform>(entity).copied()
        {
            found.push((entity, global));
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    found
}

/// Where this piece is attached to something else.
fn joint_of(world: &mut World, root: Entity) -> Option<Vec3> {
    let mine = sockets_under(world, root);
    let all: Vec<(Entity, GlobalTransform)> = world
        .query::<(Entity, &GlobalTransform, &Socket)>()
        .iter(world)
        .map(|(entity, global, _)| (entity, *global))
        .collect();
    mine.iter().find_map(|(entity, global)| {
        all.iter()
            .any(|(other, other_global)| {
                other != entity
                    && !mine.iter().any(|(m, _)| m == other)
                    && editor_prefabs::sockets::sockets_are_joined(global, other_global)
            })
            .then(|| global.translation())
    })
}

/// The socket furthest from the piece's own origin — the end that should swing.
fn far_socket(world: &mut World, root: Entity) -> Option<Vec3> {
    let at = world.get::<GlobalTransform>(root)?.translation();
    sockets_under(world, root)
        .into_iter()
        .map(|(_, global)| global.translation())
        .max_by(|a, b| a.distance(at).total_cmp(&b.distance(at)))
}

/// Fallback for the joint check: the socket nearest where the joint WAS, so a
/// joint that broke reports how far it moved instead of vanishing into `None`.
fn nearest_socket(world: &mut World, root: Entity, near: Option<Vec3>) -> Option<Vec3> {
    let near = near?;
    sockets_under(world, root)
        .into_iter()
        .map(|(_, global)| global.translation())
        .min_by(|a, b| a.distance(near).total_cmp(&b.distance(near)))
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
            // "more than before" was true of 411 walls from ONE click, which is
            // what an uncapped segment does when the cursor lands near the
            // horizon. A run is a run; a flood is a bug.
            let laid = walls.saturating_sub(before);
            check(
                world,
                laid <= editor_prefabs::paint::MAX_PIECES_PER_SEGMENT,
                &format!("and not a flood of them ({laid})"),
            );
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
        // ── Select a socket, place the next piece THERE (owner ask) ────────
        // The level-building loop: name the end of the run, pick a piece, and
        // it arrives mated — no hunting for a hover position that happens to be
        // within reach of the socket you meant.
        1010 => {
            let free = free_socket(world);
            check(world, free.is_some(), "a free socket to build from");
            if let Some((entity, at)) = free {
                world.resource_mut::<KitProbe>().socket_at = Some(at);
                world
                    .entity_mut(entity)
                    .insert(editor_core::selection::Selected);
                world.write_message(editor_core::selection::SelectionChanged);
            }
            world.resource_mut::<KitProbe>().walls_before_paint =
                instance_roots_named(world, "Wall").len();
        }
        // Park the cursor somewhere IRRELEVANT: if placement still follows the
        // cursor, the new piece lands there and the check below fails.
        1014 => {
            let size = viewport_center(world) * 2.0;
            move_cursor(world, Vec2::new(size.x * 0.2, size.y * 0.35));
        }
        1020 => tap(world, KeyCode::KeyI, "i"),
        1050 => {
            for (code, ch) in [
                (KeyCode::KeyW, "w"),
                (KeyCode::KeyA, "a"),
                (KeyCode::KeyL, "l"),
                (KeyCode::KeyL, "l"),
            ] {
                tap(world, code, ch);
            }
        }
        1080 => tap_named(world, KeyCode::Enter, Key::Enter),
        1140 => {
            let before = world.resource::<KitProbe>().walls_before_paint;
            let walls = instance_roots_named(world, "Wall");
            check(
                world,
                walls.len() == before + 1,
                &format!("one more wall ({} -> {})", before, walls.len()),
            );
            // THE assertion: it arrived at the socket that was SELECTED, not
            // under the cursor.
            let socket_at = world.resource::<KitProbe>().socket_at.unwrap_or(Vec3::ZERO);
            let nearest = walls
                .iter()
                .map(|(_, at)| at.distance(socket_at))
                .fold(f32::MAX, f32::min);
            check(
                world,
                nearest < 4.0,
                &format!("the new piece landed on the SELECTED socket ({nearest:.2}m away)"),
            );
            shot(world, "k4-spawn-at-socket");
        }
        // ── Rotating a mated piece swings it about the JOINT (owner ask) ───
        // Select the PIECE, not a socket: the joint is found for you, because
        // "rotate the thing I just attached" is the common case, and having to
        // pick the right socket first is a step nobody should need.
        1160 => {
            // The wall nearest the ORIGIN: the clean pair chained at the start,
            // not one of the painted run whose sockets touch several neighbours.
            let nearest = instance_roots_named(world, "Wall")
                .into_iter()
                .min_by(|a, b| a.1.length().total_cmp(&b.1.length()));
            let Some((entity, _)) = nearest else {
                check(world, false, "a wall to rotate");
                return;
            };
            let selected: Vec<Entity> = world
                .query_filtered::<Entity, With<editor_core::selection::Selected>>()
                .iter(world)
                .collect();
            for previous in selected {
                world
                    .entity_mut(previous)
                    .remove::<editor_core::selection::Selected>();
            }
            world
                .entity_mut(entity)
                .insert(editor_core::selection::Selected);
            world.write_message(editor_core::selection::SelectionChanged);
            let joined = joint_of(world, entity).is_some();
            check(world, joined, "the piece knows where it is joined");
            world.resource_mut::<KitProbe>().far_at = far_socket(world, entity);
        }
        // Record the pivot THE EDITOR CHOSE. A piece in a dense run can touch
        // several sockets, so a probe that guesses which joint was used
        // measures a different point than the feature did.
        1178 => {
            let pin = world.resource::<editor_core::gesture::GesturePivot>().pivot;
            world.resource_mut::<KitProbe>().joint_at = pin;
            check(world, pin.is_some(), "the rotate is pinned to that joint");
        }
        1180 => tap(world, KeyCode::KeyE, "e"),
        1190 => tap(world, KeyCode::Digit4, "4"),
        1194 => tap(world, KeyCode::Digit5, "5"),
        1200 => tap_named(world, KeyCode::Enter, Key::Enter),
        1240 => {
            let entity = match world
                .query_filtered::<Entity, (With<PrefabInstance>, With<editor_core::selection::Selected>)>()
                .iter(world)
                .next()
            {
                Some(entity) => entity,
                None => return,
            };
            let (was_joint, was_far) = {
                let probe = world.resource::<KitProbe>();
                (probe.joint_at, probe.far_at)
            };
            // Is one of this piece's sockets STILL on the point it was pinned
            // to? That is exactly what "pivot on the joint" promises.
            let moved_joint = match (was_joint, nearest_socket(world, entity, was_joint)) {
                (Some(before), Some(after)) => before.distance(after),
                _ => f32::MAX,
            };
            check(
                world,
                moved_joint < 0.05,
                &format!("rotating held the joint still ({moved_joint:.3}m)"),
            );
            let swung = match (was_far, far_socket(world, entity)) {
                (Some(before), Some(after)) => before.distance(after),
                _ => 0.0,
            };
            check(
                world,
                swung > 0.5,
                &format!("and swung the far end ({swung:.2}m)"),
            );
            shot(world, "k5-pivot-on-joint");
        }
        1300 => {
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
