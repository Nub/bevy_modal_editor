//! Blockout probe (BLOCKOUT_PROBE=1, spec §9): the greybox verbs in the REAL
//! binary. Scale is the one that decides whether hour-scale blockout is
//! possible at all — a cube has to become a wall with a gesture, not with three
//! floats typed into the inspector — so it gets end-to-end coverage from the
//! keystroke through the transaction to the statusbar readout.

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use editor_core::prelude::*;

use crate::probe_user::{click, move_cursor, shot, tap_named};

#[derive(Resource, Default)]
pub(crate) struct BlockoutProbe {
    frame: u32,
    failures: Vec<String>,
    piece: Option<SceneId>,
    named_before: usize,
    undo_before_scrub: usize,
    time_at_play: f32,
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

fn window_size(world: &mut World) -> Vec2 {
    world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .iter(world)
        .next()
        .map(|window| Vec2::new(window.width(), window.height()))
        .unwrap_or(Vec2::new(1280.0, 720.0))
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
        // Raise it NOW, not at capture time: the compositor needs frames to
        // start drawing an occluded window again.
        crate::probe_user::raise_window(world);
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
        // ── Duplicate: the verb a blockout uses more than any other ────────
        840 => {
            let before = world
                .query_filtered::<(), (With<SceneId>, With<Name>)>()
                .iter(world)
                .count();
            world.resource_mut::<BlockoutProbe>().named_before = before;
            invoke(world, "select.duplicate");
        }
        880 => {
            let before = world.resource::<BlockoutProbe>().named_before;
            let after = world
                .query_filtered::<(), (With<SceneId>, With<Name>)>()
                .iter(world)
                .count();
            check(
                world,
                after == before + 1,
                &format!("shift+d duplicated the selection ({before} then {after})"),
            );
            // The copy is what is selected, and it is handed straight to a move
            // gesture — a duplicate that lands invisibly on its original and
            // waits is not a throughput verb.
            let grabbing = !matches!(
                *world.resource::<editor_core::gesture::MoveGesture>(),
                editor_core::gesture::MoveGesture::Idle
            );
            check(world, grabbing, "the duplicate is grabbed, ready to place");
            invoke(world, "transform.axis-x");
            invoke(world, "transform.digit-3");
        }
        920 => {
            invoke(world, "transform.commit");
        }
        960 => {
            let spread: Vec<f32> = world
                .query_filtered::<&Transform, (With<SceneId>, With<Name>)>()
                .iter(world)
                .map(|transform| transform.translation.x)
                .collect();
            let moved_apart = spread.iter().any(|x| (x - 5.0).abs() < 0.01);
            check(
                world,
                moved_apart,
                &format!("the duplicate placed 3 along x, leaving the original ({spread:?})"),
            );
            shot(world, "42-blockout-duplicate");
        }
        // ── Angle snap: 15° without typing a number ───────────────────────
        // Back to the original block — the duplicate is what the grab left
        // selected, and the checks below read the original by id.
        1000 => {
            let id = world.resource::<BlockoutProbe>().piece.unwrap();
            let previous: Vec<Entity> = world
                .query_filtered::<Entity, With<Selected>>()
                .iter(world)
                .collect();
            for entity in previous {
                world.entity_mut(entity).remove::<Selected>();
            }
            if let Some(entity) = world.resource::<editor_api::edits::SceneIndex>().get(&id) {
                world.entity_mut(entity).insert(Selected);
            }
            invoke(world, "core.toggle-angle-snap");
        }
        1040 => invoke(world, "transform.rotate"),
        1080 => {
            let reads = mode_readout(world, "ROTATE");
            check(world, reads, "the statusbar reads ROTATE");
            invoke(world, "transform.digit-2");
            invoke(world, "transform.digit-0");
        }
        1120 => {
            // A TYPED angle stays exact even with the toggle on.
            let yaw =
                piece_transform(world).map(|t| t.rotation.to_euler(EulerRot::XYZ).1.to_degrees());
            check(
                world,
                yaw.is_some_and(|y| (y - 20.0).abs() < 0.05),
                &format!("a typed angle is exact, snap or no snap ({yaw:?})"),
            );
            invoke(world, "transform.cancel");
        }
        // And the drag itself lands on the step — the point of the toggle.
        1130 => invoke(world, "transform.rotate"),
        1134 => {
            // 40px of horizontal drag is 20°, which has to land on 15°.
            world
                .resource_mut::<editor_core::gesture::GestureMotion>()
                .screen = Some(Vec2::new(40.0, 0.0));
        }
        1140 => {
            let yaw =
                piece_transform(world).map(|t| t.rotation.to_euler(EulerRot::XYZ).1.to_degrees());
            check(
                world,
                yaw.is_some_and(|y| (y - 15.0).abs() < 0.05),
                &format!("a dragged rotate lands on the 15\u{b0} step ({yaw:?})"),
            );
            shot(world, "43-blockout-angle-snap");
            invoke(world, "transform.cancel");
        }
        // ── Marquee: select several things without clicking each ──────────
        // The kernel's unit tests project flat because a headless app has no
        // render target; THIS is where the real camera projection and the real
        // pointer path get exercised.
        1200 => {
            world.write_message(ActionInvoked {
                action: ActionId::new_static("select.clear"),
                args: None,
                source: InvocationSource::Test,
            });
        }
        1240 => {
            // Starting ON the ground is the point: a press arms, the release
            // decides. Requiring empty space would make a box impossible to
            // start anywhere the floor covers, which is most of the viewport.
            let size = window_size(world);
            move_cursor(world, Vec2::new(size.x * 0.68, size.y * 0.88));
            click(world, true);
        }
        1250..=1270 if frame.is_multiple_of(4) => {
            let size = window_size(world);
            let progress = (frame - 1250) as f32 / 20.0;
            move_cursor(
                world,
                Vec2::new(
                    size.x * (0.68 - 0.46 * progress),
                    size.y * (0.88 - 0.76 * progress),
                ),
            );
        }
        1274 => {
            let dragging = world.resource::<Marquee>().rect().is_some();
            check(world, dragging, "a drag past the threshold IS a box");
            shot(world, "44-blockout-marquee");
        }
        1280 => click(world, false),
        1320 => {
            let selected = world
                .query_filtered::<(), (With<Selected>, With<SceneId>)>()
                .iter(world)
                .count();
            check(
                world,
                selected > 1,
                &format!("a box over the scene selected {selected} things at once"),
            );
            let gone = world.resource::<Marquee>().rect().is_none();
            check(world, gone, "and the box is put away on release");
        }
        // A plain click still selects exactly one thing — the release-decides
        // rewrite must not have cost the ordinary click.
        1360 => {
            let size = window_size(world);
            move_cursor(world, Vec2::new(size.x * 0.5, size.y * 0.6));
            click(world, true);
        }
        1364 => click(world, false),
        1400 => {
            let selected = world
                .query_filtered::<(), (With<Selected>, With<SceneId>)>()
                .iter(world)
                .count();
            check(
                world,
                selected == 1,
                &format!("a click still selects exactly one ({selected})"),
            );
        }
        // ── The first track: key a pose, key another, scrub between them ──
        1500 => {
            let id = world.resource::<BlockoutProbe>().piece.unwrap();
            let previous: Vec<Entity> = world
                .query_filtered::<Entity, With<Selected>>()
                .iter(world)
                .collect();
            for entity in previous {
                world.entity_mut(entity).remove::<Selected>();
            }
            if let Some(entity) = world.resource::<editor_api::edits::SceneIndex>().get(&id) {
                world.entity_mut(entity).insert(Selected);
                // A known pose to key FROM.
                if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                    transform.translation = Vec3::new(0.0, 1.0, 0.0);
                }
            }
        }
        1520 => invoke(world, "anim.key"),
        // Move time FIRST and let evaluation settle, then pose: a pose set in
        // the same frame the playhead moves is overwritten by the evaluation
        // that move triggers, which is correct and would make this test lie.
        1540 => world.resource_mut::<editor_scene::anim::Playhead>().time = 2.0,
        1550 => {
            let id = world.resource::<BlockoutProbe>().piece.unwrap();
            if let Some(entity) = world.resource::<editor_api::edits::SceneIndex>().get(&id)
                && let Some(mut transform) = world.get_mut::<Transform>(entity)
            {
                transform.translation = Vec3::new(0.0, 5.0, 0.0);
            }
        }
        1560 => invoke(world, "anim.key"),
        1580 => {
            let duration = world.resource::<editor_scene::anim::Timeline>().duration();
            check(
                world,
                (duration - 2.0).abs() < 1e-3,
                &format!("the timeline runs as long as its last key ({duration})"),
            );
            // Scrub to the middle: the thing has to be halfway between poses.
            world.resource_mut::<editor_scene::anim::Playhead>().time = 1.0;
        }
        1600 => {
            let height = piece_transform(world).map(|transform| transform.translation.y);
            check(
                world,
                height.is_some_and(|y| (y - 3.0).abs() < 0.05),
                &format!("scrubbing to the middle puts it between the poses ({height:?})"),
            );
            shot(world, "45-blockout-timeline");
        }
        // Evaluation is NOT history: a scrub must not have queued a single
        // undoable edit, or one drag of the playhead would bury the real work.
        1620 => {
            let depth = world.resource::<editor_core::edits::History>().undo_depth();
            world.resource_mut::<BlockoutProbe>().undo_before_scrub = depth;
            world.resource_mut::<editor_scene::anim::Playhead>().time = 0.4;
        }
        1640 => {
            world.resource_mut::<editor_scene::anim::Playhead>().time = 1.6;
        }
        1660 => {
            let before = world.resource::<BlockoutProbe>().undo_before_scrub;
            let now = world.resource::<editor_core::edits::History>().undo_depth();
            check(
                world,
                now == before,
                &format!("scrubbing left history alone ({before} then {now})"),
            );
        }
        // And it PLAYS: time moves on its own.
        // Rewind before playing: the timeline LOOPS, so starting mid-run and
        // asking "did time increase" is a question that wraps to no.
        1670 => invoke(world, "anim.rewind"),
        1680 => invoke(world, "anim.play"),
        1700 => {
            let playing = world.resource::<editor_scene::anim::Playhead>().playing;
            check(world, playing, "play starts the playhead");
            world.resource_mut::<BlockoutProbe>().time_at_play =
                world.resource::<editor_scene::anim::Playhead>().time;
        }
        1740 => {
            let started = world.resource::<BlockoutProbe>().time_at_play;
            let now = world.resource::<editor_scene::anim::Playhead>().time;
            check(
                world,
                now > started,
                &format!("time actually moves while playing ({started} then {now})"),
            );
            invoke(world, "anim.rewind");
        }
        1780 => {
            let playhead = world.resource::<editor_scene::anim::Playhead>();
            let (time, playing) = (playhead.time, playhead.playing);
            check(
                world,
                time == 0.0 && !playing,
                "rewind stops it and puts it back to the start",
            );
        }
        // The keys have to reach DISK, or the animation is a session-long demo.
        1800 => {
            let path = world
                .resource::<editor_scene::anim::Timeline>()
                .path
                .clone();
            let on_disk = editor_scene::anim::load_timeline(&path);
            match on_disk {
                Ok(tracks) => {
                    check(
                        world,
                        tracks.len() >= 3,
                        &format!(
                            "the keyed tracks reached the file ({} tracks)",
                            tracks.len()
                        ),
                    );
                    // And they came back as the SAME animation, not just as rows.
                    let sampled = tracks
                        .iter()
                        .find(|track| track.path == "translation.y")
                        .and_then(|track| track.sample(1.0));
                    check(
                        world,
                        sampled.is_some_and(|y| (y - 3.0).abs() < 0.05),
                        &format!("and reloaded they sample the same pose ({sampled:?})"),
                    );
                }
                Err(error) => check(
                    world,
                    false,
                    &format!("the timeline file loads back ({error:?})"),
                ),
            }
        }
        1820 => {
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
