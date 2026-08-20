//! Blockout probe (BLOCKOUT_PROBE=1, spec §9): the greybox verbs in the REAL
//! binary. Scale is the one that decides whether hour-scale blockout is
//! possible at all — a cube has to become a wall with a gesture, not with three
//! floats typed into the inspector — so it gets end-to-end coverage from the
//! keystroke through the transaction to the statusbar readout.

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use editor_core::prelude::*;

use crate::probe_user::{shot, tap_named};

#[derive(Resource, Default)]
pub(crate) struct BlockoutProbe {
    frame: u32,
    failures: Vec<String>,
    piece: Option<SceneId>,
}

fn check(world: &mut World, ok: bool, what: &str) {
    if ok {
        info!("BLOCKOUT-PROBE PASS: {what}");
    } else {
        error!("BLOCKOUT-PROBE FAIL: {what}");
        world
            .resource_mut::<BlockoutProbe>()
            .failures
            .push(what.to_string());
    }
}

fn invoke(world: &mut World, action: &'static str) {
    world.write_message(ActionInvoked {
        action: ActionId::new_static(action),
        args: None,
        source: InvocationSource::Test,
    });
}

fn piece_transform(world: &mut World) -> Option<Transform> {
    let id = world.resource::<BlockoutProbe>().piece?;
    let entity = world.resource::<editor_api::edits::SceneIndex>().get(&id)?;
    world.get::<Transform>(entity).copied()
}

/// Whatever the statusbar is currently showing as the mode — the gesture has to
/// name ITSELF there, or the user cannot tell a scale from a move.
fn mode_readout(world: &mut World, starts_with: &str) -> bool {
    world
        .query::<&Text>()
        .iter(world)
        .any(|text| text.0.starts_with(starts_with))
}

pub(crate) fn probe_blockout(world: &mut World) {
    use bevy::reflect::PartialReflect;
    use editor_api::edits::{EditQueue, Op, Transaction};

    world.resource_mut::<BlockoutProbe>().frame += 1;
    let frame = world.resource::<BlockoutProbe>().frame;
    if frame == 1 {
        let _ = std::fs::create_dir_all(crate::probe_user::SHOT_DIR);
        info!("BLOCKOUT-PROBE armed");
    }
    match frame {
        60 => tap_named(world, KeyCode::Enter, Key::Enter),
        120 => invoke(world, "core.toggle-editor"),
        // One greybox block, selected — the thing every blockout starts from.
        200 => {
            let piece = SceneId::random();
            world.resource_mut::<BlockoutProbe>().piece = Some(piece);
            world.resource_mut::<EditQueue>().0.push(Transaction {
                label: "probe block".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: piece,
                    components: vec![
                        Box::new(Transform::from_xyz(2.0, 1.0, 2.0)).into_partial_reflect(),
                        Box::new(Name::new("probe block")).into_partial_reflect(),
                    ],
                }],
            });
        }
        240 => {
            let id = world.resource::<BlockoutProbe>().piece.unwrap();
            if let Some(entity) = world.resource::<editor_api::edits::SceneIndex>().get(&id) {
                world.entity_mut(entity).insert(Selected);
            }
            let scale = piece_transform(world).map(|t| t.scale);
            check(world, scale == Some(Vec3::ONE), "the block starts at 1x");
        }
        // s 2 ⏎ — a typed factor, exactly twice as big.
        280 => invoke(world, "transform.scale"),
        320 => {
            let reads = mode_readout(world, "SCALE");
            check(world, reads, "the statusbar reads SCALE");
            invoke(world, "transform.digit-2");
        }
        360 => {
            let scale = piece_transform(world).map(|t| t.scale);
            check(
                world,
                scale == Some(Vec3::splat(2.0)),
                "s 2 doubles the block live, before commit",
            );
            shot(world, "40-blockout-scale-uniform");
            invoke(world, "transform.commit");
        }
        // s x 4 ⏎ — the cube-into-a-wall move.
        400 => invoke(world, "transform.scale"),
        440 => invoke(world, "transform.axis-x"),
        480 => {
            let reads = mode_readout(world, "SCALE · X");
            check(world, reads, "the statusbar names the constrained axis");
            invoke(world, "transform.digit-4");
        }
        520 => {
            let scale = piece_transform(world).map(|t| t.scale);
            check(
                world,
                scale == Some(Vec3::new(8.0, 2.0, 2.0)),
                &format!("s x 4 stretches ONE axis: {scale:?}"),
            );
            let position = piece_transform(world).map(|t| t.translation);
            check(
                world,
                position == Some(Vec3::new(2.0, 1.0, 2.0)),
                &format!("a single selection grows in place: {position:?}"),
            );
            shot(world, "41-blockout-scale-axis");
            invoke(world, "transform.commit");
        }
        // Backspacing a typed factor returns to 1x rather than collapsing the
        // block to nothing — the state you pass through while fixing a typo.
        528 => invoke(world, "transform.scale"),
        532 => invoke(world, "transform.digit-9"),
        536 => invoke(world, "transform.digit-erase"),
        540 => {
            let scale = piece_transform(world).map(|t| t.scale);
            check(
                world,
                scale == Some(Vec3::new(8.0, 2.0, 2.0)),
                &format!("backspacing a typed factor holds at 1x, not zero: {scale:?}"),
            );
            invoke(world, "transform.cancel");
        }
        // Esc mid-gesture restores the size exactly and leaves no history.
        560 => invoke(world, "transform.scale"),
        600 => invoke(world, "transform.digit-9"),
        640 => invoke(world, "transform.cancel"),
        680 => {
            let scale = piece_transform(world).map(|t| t.scale);
            check(
                world,
                scale == Some(Vec3::new(8.0, 2.0, 2.0)),
                &format!("Esc restores the pre-gesture size: {scale:?}"),
            );
        }
        // Each committed scale was ONE history entry, so one undo undoes one.
        720 => invoke(world, "core.undo"),
        760 => {
            let scale = piece_transform(world).map(|t| t.scale);
            check(
                world,
                scale == Some(Vec3::splat(2.0)),
                &format!("one undo takes back the whole axis scale: {scale:?}"),
            );
            invoke(world, "core.undo");
        }
        800 => {
            let scale = piece_transform(world).map(|t| t.scale);
            check(
                world,
                scale == Some(Vec3::ONE),
                &format!("a second undo takes back the uniform scale: {scale:?}"),
            );
        }
        860 => {
            let failures = world.resource::<BlockoutProbe>().failures.clone();
            if failures.is_empty() {
                info!("BLOCKOUT-PROBE PASS: blockout verbs end-to-end");
                world.write_message(AppExit::Success);
            } else {
                error!("BLOCKOUT-PROBE FAILED: {failures:?}");
                world.write_message(AppExit::error());
            }
        }
        _ => {}
    }
}
