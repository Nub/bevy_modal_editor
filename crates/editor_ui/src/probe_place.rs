//! Placement probe (PLACE_PROBE=1, spec §9 assisted layout, M4-ACCEPTANCE D9):
//! placing a socketed prefab from the palette has to MATE with what is already
//! there. The palette computed that mate all along and spawned at the raw
//! cursor point anyway, spending the result only on the wording of the toast —
//! so the editor announced a snap it had not performed. The kit probe missed it
//! because it presses `o` next, which mates through a different path.
//!
//! Two placements in a row with nothing moved between them is the sharpest form
//! of the test: both resolve the same placement point, so before the fix the
//! second wall lands exactly on top of the first, and after it the two sit one
//! length apart with their sockets touching.

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use editor_core::prelude::*;
use editor_prefabs::sockets::Socket;
use editor_prefabs::{PrefabInstance, PrefabLibrary};
use editor_scene::PrefabStamped;

use crate::probe_user::{shot, tap, tap_named};

#[derive(Resource, Default)]
pub(crate) struct PlaceProbe {
    frame: u32,
    failures: Vec<String>,
    first_at: Option<Vec3>,
}

fn check(world: &mut World, ok: bool, what: &str) {
    if ok {
        info!("PLACE-PROBE PASS: {what}");
    } else {
        error!("PLACE-PROBE FAIL: {what}");
        world
            .resource_mut::<PlaceProbe>()
            .failures
            .push(what.to_string());
    }
}

fn walls(world: &mut World) -> Vec<(Entity, Vec3)> {
    world
        .query_filtered::<(Entity, &Name, &Transform), (With<PrefabInstance>, Without<PrefabStamped>)>()
        .iter(world)
        .filter(|(_, name, _)| name.as_str() == "Wall")
        .map(|(entity, _, transform)| (entity, transform.translation))
        .collect()
}

/// Every socket under a root, in world space.
fn sockets_of(world: &mut World, root: Entity) -> Vec<Vec3> {
    let mut stack = vec![root];
    let mut members = Vec::new();
    while let Some(current) = stack.pop() {
        members.push(current);
        if let Some(children) = world.get::<Children>(current) {
            stack.extend(children.iter());
        }
    }
    members
        .into_iter()
        .filter(|entity| world.get::<Socket>(*entity).is_some())
        .filter_map(|entity| {
            world
                .get::<GlobalTransform>(entity)
                .map(|g| g.translation())
        })
        .collect()
}

/// Mated = one socket from each root sitting in the same place.
fn mated(world: &mut World, a: Entity, b: Entity) -> bool {
    let left = sockets_of(world, a);
    let right = sockets_of(world, b);
    left.iter()
        .any(|l| right.iter().any(|r| l.distance(*r) < 0.05))
}

/// Type "wall" into the open palette.
fn type_wall(world: &mut World) {
    for (code, ch) in [
        (KeyCode::KeyW, "w"),
        (KeyCode::KeyA, "a"),
        (KeyCode::KeyL, "l"),
        (KeyCode::KeyL, "l"),
    ] {
        tap(world, code, ch);
    }
}

pub(crate) fn probe_place(world: &mut World) {
    world.resource_mut::<PlaceProbe>().frame += 1;
    let frame = world.resource::<PlaceProbe>().frame;
    if frame == 1 {
        let _ = std::fs::create_dir_all(crate::probe_user::SHOT_DIR);
        info!("PLACE-PROBE armed");
    }
    match frame {
        60 => tap_named(world, KeyCode::Enter, Key::Enter),
        120 => invoke_toggle(world),
        180 => {
            let has_wall = world
                .resource::<PrefabLibrary>()
                .prefabs
                .values()
                .any(|def| def.name == "Wall");
            check(world, has_wall, "the demo Wall prefab is in the library");
        }
        // ── First wall from the palette ────────────────────────────────────
        200 => tap(world, KeyCode::KeyI, "i"),
        240 => type_wall(world),
        280 => tap_named(world, KeyCode::Enter, Key::Enter),
        340 => {
            let placed = walls(world);
            if placed.len() != 1 {
                check(world, false, "one Wall placed from the palette");
                return;
            }
            world.resource_mut::<PlaceProbe>().first_at = Some(placed[0].1);
            check(world, true, "one Wall placed from the palette");
            shot(world, "50-place-first");
        }
        // ── Second wall from the palette, nothing moved in between ─────────
        // Placement auto-selects what it placed, and `i` WITH a selection is
        // the add-component palette instead of the insert one.
        380 => {
            world.write_message(ActionInvoked {
                action: ActionId::new_static("select.clear"),
                args: None,
                source: InvocationSource::Test,
            });
        }
        400 => tap(world, KeyCode::KeyI, "i"),
        440 => type_wall(world),
        480 => tap_named(world, KeyCode::Enter, Key::Enter),
        540 => {
            let placed = walls(world);
            if placed.len() != 2 {
                check(world, false, "a second Wall placed from the palette");
                return;
            }
            let first_at = world.resource::<PlaceProbe>().first_at.unwrap_or_default();
            let second = placed
                .iter()
                .find(|(_, at)| at.distance(first_at) > 1e-4)
                .copied();
            // Both placements resolve the SAME point, so an unsnapped second
            // wall lands exactly on the first.
            check(
                world,
                second.is_some(),
                "the second Wall did not land on top of the first",
            );
            // Two walls stacked in the same spot have coincident sockets, which
            // would read as "mated" — the pieces have to be apart AND joined.
            let is_mated = second.is_some() && mated(world, placed[0].0, placed[1].0);
            check(
                world,
                is_mated,
                "palette placement MATES to a nearby socket, with no o press",
            );
            if let Some((_, at)) = second {
                let spacing = at.distance(first_at);
                check(
                    world,
                    (spacing - 2.0).abs() < 0.05,
                    &format!("the mated wall sits one length away ({spacing:.3})"),
                );
            }
            shot(world, "51-place-mated");
        }
        600 => {
            let failures = world.resource::<PlaceProbe>().failures.clone();
            if failures.is_empty() {
                info!("PLACE-PROBE PASS: palette placement mates");
                world.write_message(AppExit::Success);
            } else {
                error!("PLACE-PROBE FAILED: {failures:?}");
                world.write_message(AppExit::error());
            }
        }
        _ => {}
    }
}

fn invoke_toggle(world: &mut World) {
    world.write_message(ActionInvoked {
        action: ActionId::new_static("core.toggle-editor"),
        args: None,
        source: InvocationSource::Test,
    });
}
