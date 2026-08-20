//! Blockout probe (BLOCKOUT_PROBE=1, spec §9): the greybox verbs in the REAL
//! binary. Scale is the one that decides whether hour-scale blockout is
//! possible at all — a cube has to become a wall with a gesture, not with three
//! floats typed into the inspector — so it gets end-to-end coverage from the
//! keystroke through the transaction to the statusbar readout.

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use editor_core::prelude::*;

use crate::probe_handson::{screen_position_of, type_word};
use crate::probe_user::{click, move_cursor, shot, tap, tap_named};

#[derive(Resource, Default)]
pub(crate) struct BlockoutProbe {
    frame: u32,
    failures: Vec<String>,
    piece: Option<SceneId>,
    named_before: usize,
    undo_before_scrub: usize,
    time_at_play: f32,
    linear_midpoint: f32,
    events_seen: usize,
    volume_at: Option<Vec3>,
    actor_home: Option<Vec3>,
    actor_body: Option<Entity>,
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

/// Read a bool field off a GAME component, by name, through the type registry.
///
/// The editor must not depend on the game — that would invert the whole point —
/// so the probe checks the game reacted the same way the editor does anything
/// to a game type: by reflection, knowing only a name.
fn game_flag(world: &mut World, type_suffix: &str, field: &str) -> Option<bool> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let registration = registry
        .iter()
        .find(|registration| registration.type_info().type_path().ends_with(type_suffix))?;
    let reflect_component = registration.data::<bevy::ecs::reflect::ReflectComponent>()?;
    let entities: Vec<Entity> = world.iter_entities().map(|entity| entity.id()).collect();
    let parsed = bevy::reflect::ParsedPath::parse(field).ok()?;
    for entity in entities {
        let Ok(entity_ref) = world.get_entity(entity) else {
            continue;
        };
        let Some(component) = reflect_component.reflect(entity_ref) else {
            continue;
        };
        if let Ok(element) = parsed.reflect_element(component.as_partial_reflect())
            && let Some(value) = element.try_downcast_ref::<bool>()
            && *value
        {
            return Some(true);
        }
    }
    Some(false)
}

/// Read a String field off a GAME component, by name. Same reflection-only
/// contract as `game_flag`.
fn game_string(world: &mut World, type_suffix: &str, field: &str) -> Option<String> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let registration = registry
        .iter()
        .find(|registration| registration.type_info().type_path().ends_with(type_suffix))?;
    let reflect_component = registration.data::<bevy::ecs::reflect::ReflectComponent>()?;
    let parsed = bevy::reflect::ParsedPath::parse(field).ok()?;
    let entities: Vec<Entity> = world.iter_entities().map(|entity| entity.id()).collect();
    for entity in entities {
        let Ok(entity_ref) = world.get_entity(entity) else {
            continue;
        };
        let Some(value) = reflect_component.reflect(entity_ref) else {
            continue;
        };
        if let Ok(element) = parsed.element::<String>(value.as_partial_reflect()) {
            return Some(element.clone());
        }
    }
    None
}

/// Walk the actor into the volume, or home again. x/z only: the walk system
/// pins y to standing height on every live frame, so a posed y is silently
/// reverted — and the check that depended on it would then fail for a reason
/// that is not the feature.
fn pose_actor(world: &mut World, inside: bool) {
    let (body, to) = {
        let probe = world.resource::<BlockoutProbe>();
        (
            probe.actor_body,
            if inside {
                probe.volume_at
            } else {
                probe.actor_home
            },
        )
    };
    if let Some((body, to)) = body.zip(to)
        && let Some(mut transform) = world.get_mut::<Transform>(body)
    {
        transform.translation.x = to.x;
        transform.translation.z = to.z;
    }
}
/// The type path of a registered component, found by the tail of its name.
/// The editor knows game types only as strings — that is the whole contract.
fn game_type_path(world: &mut World, suffix: &str) -> Option<String> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    registry
        .iter()
        .map(|registration| registration.type_info().type_path())
        .find(|path| path.ends_with(suffix))
        .map(|path| path.to_string())
}

/// The largest value of a f32 field across every entity carrying a component,
/// again by name only.
fn game_number(world: &mut World, type_suffix: &str, field: &str) -> Option<f32> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let registration = registry
        .iter()
        .find(|registration| registration.type_info().type_path().ends_with(type_suffix))?;
    let reflect_component = registration.data::<bevy::ecs::reflect::ReflectComponent>()?;
    let parsed = bevy::reflect::ParsedPath::parse(field).ok()?;
    let entities: Vec<Entity> = world.iter_entities().map(|entity| entity.id()).collect();
    let mut best: Option<f32> = None;
    for entity in entities {
        let Ok(entity_ref) = world.get_entity(entity) else {
            continue;
        };
        let Some(component) = reflect_component.reflect(entity_ref) else {
            continue;
        };
        if let Ok(element) = parsed.reflect_element(component.as_partial_reflect())
            && let Some(value) = element.try_downcast_ref::<f32>()
        {
            best = Some(best.map_or(*value, |current: f32| current.max(*value)));
        }
    }
    best
}

/// The entity carrying a GAME component, found by type-path suffix — plus its
/// `SceneId`, which is how an edit addresses it.
fn game_entity(world: &mut World, type_suffix: &str) -> Option<(Entity, SceneId)> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let registration = registry
        .iter()
        .find(|registration| registration.type_info().type_path().ends_with(type_suffix))?;
    let reflect_component = registration.data::<bevy::ecs::reflect::ReflectComponent>()?;
    let entities: Vec<Entity> = world.iter_entities().map(|entity| entity.id()).collect();
    for entity in entities {
        let Ok(entity_ref) = world.get_entity(entity) else {
            continue;
        };
        // A `SceneId` is the difference between AUTHORED content and the insert
        // ghost, which carries the same components and would otherwise be the
        // first thing found.
        if reflect_component.reflect(entity_ref).is_some()
            && let Some(id) = entity_ref.get::<SceneId>().copied()
        {
            return Some((entity, id));
        }
    }
    None
}

/// Just the entity — for a marker the GAME spawns, which has no `SceneId`
/// because it is not authored content.
fn game_marker(world: &mut World, type_suffix: &str) -> Option<Entity> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let registration = registry
        .iter()
        .find(|registration| registration.type_info().type_path().ends_with(type_suffix))?;
    let reflect_component = registration.data::<bevy::ecs::reflect::ReflectComponent>()?;
    let entities: Vec<Entity> = world.iter_entities().map(|entity| entity.id()).collect();
    entities.into_iter().find(|entity| {
        world
            .get_entity(*entity)
            .ok()
            .and_then(|entity_ref| reflect_component.reflect(entity_ref))
            .is_some()
    })
}

/// Count particles the way the editor knows anything about a game: by name.
fn particle_count(world: &mut World) -> usize {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let Some(registration) = registry
        .iter()
        .find(|registration| registration.type_info().type_path().ends_with("::Particle"))
    else {
        return 0;
    };
    let Some(reflect_component) = registration.data::<bevy::ecs::reflect::ReflectComponent>()
    else {
        return 0;
    };
    let entities: Vec<Entity> = world.iter_entities().map(|entity| entity.id()).collect();
    entities
        .into_iter()
        .filter(|entity| {
            world
                .get_entity(*entity)
                .ok()
                .and_then(|entity_ref| reflect_component.reflect(entity_ref))
                .is_some()
        })
        .count()
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

/// Count timeline events as they fire — the probe is the "game" reacting.
pub(crate) fn count_timeline_events(
    mut reader: MessageReader<editor_scene::anim::TimelineEvent>,
    mut probe: ResMut<BlockoutProbe>,
) {
    for event in reader.read() {
        probe.events_seen += 1;
        info!(
            "BLOCKOUT-PROBE saw event {:?} at {}",
            event.name, event.time
        );
    }
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
                Ok((tracks, _events)) => {
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
        // ── The track view: keys you can SEE ──────────────────────────────
        1830 => invoke(world, "timeline.toggle"),
        1870 => {
            let open = world
                .resource::<crate::timeline_panel::TimelinePanelState>()
                .open;
            check(world, open, "the timeline panel opens");
            let rows = world
                .query::<&crate::timeline_panel::TrackStrip>()
                .iter(world)
                .count();
            check(
                world,
                rows == 3,
                &format!("one strip per keyed track ({rows})"),
            );
        }
        1890 => {
            // WHERE a key sits is the part that can silently be wrong: a mark
            // at the wrong fraction still looks like a timeline.
            let marks: Vec<(f32, f32)> = world
                .query::<&crate::timeline_panel::KeyMark>()
                .iter(world)
                .map(|mark| (mark.time, mark.fraction))
                .collect();
            check(
                world,
                marks.len() == 6,
                &format!("two keys on each of three tracks ({})", marks.len()),
            );
            // Duration is 2s, so a key at 0s sits at the left edge and one at
            // 2s sits at the right.
            let placed_right = marks
                .iter()
                .all(|(time, fraction)| (time / 2.0 - fraction).abs() < 1e-3);
            check(
                world,
                placed_right,
                &format!("each mark sits at its own moment ({marks:?})"),
            );
            shot(world, "46-blockout-track-view");
            // A window screenshot of chrome is a black frame whenever the
            // terminal is in front, so the geometry gets checked instead: a
            // panel of zero height, or one sitting under the status bar or off
            // the screen, is what a layout bug actually looks like.
            // ComputedNode reports PHYSICAL pixels; the window reports logical.
            // On a retina display those differ by 2, which is enough to make a
            // correctly placed panel look like it is off the screen.
            let scale = world
                .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
                .iter(world)
                .next()
                .map(|window| window.scale_factor())
                .unwrap_or(1.0);
            let size = window_size(world) * scale;
            let placed = world
                .query_filtered::<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<crate::timeline_panel::TimelinePanel>>()
                .iter(world)
                .next()
                .map(|(node, transform)| {
                    let extent = node.size();
                    let centre = transform.translation;
                    (extent, centre)
                });
            match placed {
                Some((extent, centre)) => {
                    check(
                        world,
                        extent.x > 200.0 && extent.y > 20.0,
                        &format!("the panel has a real size ({extent:?})"),
                    );
                    let bottom = centre.y + extent.y * 0.5;
                    check(
                        world,
                        bottom < size.y - 20.0 * scale,
                        &format!("and sits clear of the status bar ({bottom} of {})", size.y),
                    );
                    let (left, right) = (centre.x - extent.x * 0.5, centre.x + extent.x * 0.5);
                    check(
                        world,
                        left > 0.0 && right < size.x,
                        &format!("and inside the window ({left}..{right} of {})", size.x),
                    );
                }
                None => check(world, false, "the panel is laid out at all"),
            }
        }
        1910 => {
            // The cursor tracks time rather than being redrawn from scratch.
            world.resource_mut::<editor_scene::anim::Playhead>().time = 1.5;
        }
        1930 => {
            let left = world
                .query_filtered::<&Node, With<crate::timeline_panel::TimelineCursor>>()
                .iter(world)
                .next()
                .map(|node| node.left);
            check(
                world,
                matches!(left, Some(bevy::ui::Val::Percent(p)) if (p - 75.0).abs() < 0.5),
                &format!("the cursor sits where time is ({left:?})"),
            );
        }
        // ── Keying a field that is NOT a position ─────────────────────────
        // Spec §9 promises any reflected property. Scale is the proof that the
        // address, not the meaning, is what a track holds.
        // The inspector shows the SELECTION, and its key affordances address
        // whatever it is showing — so the thing being posed has to be the thing
        // being inspected. The marquee and the click before this left something
        // else selected, and the keys went there.
        1936 => {
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
            // The inspector rebuilds on the MESSAGE, not on the component:
            // inserting Selected quietly is enough for a gesture and not for a
            // panel, which is why the rows kept addressing the old entity.
            world.write_message(SelectionChanged);
        }
        1940 => {
            let id = world.resource::<BlockoutProbe>().piece.unwrap();
            if let Some(entity) = world.resource::<editor_api::edits::SceneIndex>().get(&id)
                && let Some(mut transform) = world.get_mut::<Transform>(entity)
            {
                transform.scale = Vec3::splat(1.0);
            }
            world.resource_mut::<editor_scene::anim::Playhead>().time = 0.0;
        }
        1950 => {
            let affordances: Vec<Entity> = world
                .query_filtered::<Entity, With<crate::inspector::KeyFieldAffordance>>()
                .iter(world)
                .collect();
            check(
                world,
                !affordances.is_empty(),
                &format!(
                    "numeric rows carry a key affordance ({})",
                    affordances.len()
                ),
            );
            // Key scale.x specifically, by address.
            let target = world
                .query::<(Entity, &crate::inspector::KeyFieldAffordance)>()
                .iter(world)
                .find(|(_, affordance)| affordance.paths.iter().any(|path| path == "scale.x"))
                .map(|(entity, _)| entity);
            match target {
                Some(entity) => {
                    let center = crate::probe_handson::ui_center(world, entity)
                        .unwrap_or(Vec2::new(10.0, 10.0));
                    move_cursor(world, center);
                    click(world, true);
                    click(world, false);
                }
                None => check(world, false, "the inspector offers a scale.x row to key"),
            }
        }
        1960 => {
            let keyed = world
                .resource::<editor_scene::anim::Timeline>()
                .tracks
                .iter()
                .any(|track| track.path == "scale.x");
            check(world, keyed, "pressing it keyed scale.x, not a position");
        }
        // A second key at a later moment, then scrub between: the field has to
        // MOVE, which is what proves the track drives it.
        1966 => {
            world.resource_mut::<editor_scene::anim::Playhead>().time = 2.0;
        }
        1972 => {
            let id = world.resource::<BlockoutProbe>().piece.unwrap();
            if let Some(entity) = world.resource::<editor_api::edits::SceneIndex>().get(&id)
                && let Some(mut transform) = world.get_mut::<Transform>(entity)
            {
                transform.scale.x = 5.0;
            }
        }
        1978 => {
            // Re-assert the pose in the SAME frame as the press. Evaluation and
            // the probe are both in Sync with no order between them, so a pose
            // set several frames earlier can be overwritten before it is keyed
            // — which is exactly what happened, and recorded a key of 1.0.
            let id = world.resource::<BlockoutProbe>().piece.unwrap();
            if let Some(entity) = world.resource::<editor_api::edits::SceneIndex>().get(&id)
                && let Some(mut transform) = world.get_mut::<Transform>(entity)
            {
                transform.scale.x = 5.0;
            }
            let target = world
                .query::<(Entity, &crate::inspector::KeyFieldAffordance)>()
                .iter(world)
                .find(|(_, affordance)| affordance.paths.iter().any(|path| path == "scale.x"))
                .map(|(entity, _)| entity);
            if let Some(entity) = target {
                let center =
                    crate::probe_handson::ui_center(world, entity).unwrap_or(Vec2::new(10.0, 10.0));
                move_cursor(world, center);
                click(world, true);
                click(world, false);
            }
        }
        1990 => world.resource_mut::<editor_scene::anim::Playhead>().time = 1.0,
        2000 => {
            let scale = piece_transform(world).map(|transform| transform.scale.x);
            check(
                world,
                scale.is_some_and(|x| (x - 3.0).abs() < 0.05),
                &format!("scrubbing drives the keyed SCALE, halfway ({scale:?})"),
            );
        }
        // ── Events: the sequencer's second job ────────────────────────────
        2010 => {
            world.resource_mut::<editor_scene::anim::Playhead>().time = 1.0;
            invoke(world, "timeline.event");
        }
        2030 => {
            let open = world
                .resource::<editor_prefabs::authoring::GroupPrompt>()
                .open;
            check(world, open, "adding an event asks for its name");
            // An event's whole content is its name, so it is typed, not generated.
            for (code, ch) in [
                (KeyCode::KeyD, "d"),
                (KeyCode::KeyU, "u"),
                (KeyCode::KeyS, "s"),
                (KeyCode::KeyT, "t"),
            ] {
                tap(world, code, ch);
            }
        }
        2060 => tap_named(world, KeyCode::Enter, Key::Enter),
        2090 => {
            let events = world
                .resource::<editor_scene::anim::Timeline>()
                .events
                .clone();
            check(
                world,
                events.len() == 1 && events[0].name == "dust",
                &format!("the named event landed at the playhead ({events:?})"),
            );
            check(
                world,
                events.first().is_some_and(|e| (e.time - 1.0).abs() < 0.01),
                "at the moment time was parked on",
            );
        }
        // Scrubbing must NOT fire it: dragging through a footstep should not
        // play forty footsteps.
        2110 => {
            world.resource_mut::<BlockoutProbe>().events_seen = 0;
            world.resource_mut::<editor_scene::anim::Playhead>().time = 0.0;
        }
        2120 => world.resource_mut::<editor_scene::anim::Playhead>().time = 1.8,
        2130 => {
            let seen = world.resource::<BlockoutProbe>().events_seen;
            check(
                world,
                seen == 0,
                &format!("scrubbing past an event does not fire it ({seen})"),
            );
        }
        // Playing across it fires it exactly once.
        2140 => {
            world.resource_mut::<editor_scene::anim::Playhead>().time = 0.9;
            world.resource_mut::<BlockoutProbe>().events_seen = 0;
            invoke(world, "anim.play");
        }
        2200 => {
            let seen = world.resource::<BlockoutProbe>().events_seen;
            check(
                world,
                seen == 1,
                &format!("playing across it fires it ONCE ({seen})"),
            );
            invoke(world, "anim.rewind");
        }
        // The event shows in the track view, named, at its own moment.
        2210 => {
            let marks: Vec<(f32, f32, String)> = world
                .query::<&crate::timeline_panel::EventMark>()
                .iter(world)
                .map(|mark| (mark.time, mark.fraction, mark.name.clone()))
                .collect();
            check(
                world,
                marks.len() == 1 && marks[0].2 == "dust",
                &format!("the event shows in the track view, named ({marks:?})"),
            );
            let duration = world.resource::<editor_scene::anim::Timeline>().duration();
            check(
                world,
                marks
                    .first()
                    .is_some_and(|(time, fraction, _)| (time / duration - fraction).abs() < 1e-3),
                "and sits at the moment it fires",
            );
            shot(world, "47-blockout-events");
        }
        // ── The other half of the contract: the GAME answers it ───────────
        2220 => {
            // A second event the reference game knows the meaning of.
            world
                .resource_mut::<editor_scene::anim::Timeline>()
                .events
                .push(editor_scene::anim::EventMarker {
                    // Early in the run: the check must not race the playhead.
                    time: 0.15,
                    name: "spin".into(),
                });
            let spinning = game_flag(world, "Spinner", "enabled").unwrap_or(false);
            check(world, !spinning, "nothing is spinning to begin with");
            world.resource_mut::<editor_scene::anim::Playhead>().time = 0.0;
        }
        2230 => invoke(world, "anim.play"),
        2280 => {
            let spinning = game_flag(world, "Spinner", "enabled").unwrap_or(false);
            check(
                world,
                spinning,
                "playing past the event made the GAME act on it",
            );
            invoke(world, "anim.rewind");
        }
        // ── An EFFECT on the timeline ─────────────────────────────────────
        // The look is game data on a component, so it inherits the whole
        // authoring stack for nothing: a track addresses it exactly as it
        // addresses a position, and the sequencer drives it. Bloom over time
        // with no effects-specific animation code anywhere.
        2290 => {
            // Found by NAME, like everything else the editor knows about a game.
            let type_path = game_type_path(world, "PostProcess");
            let target = type_path.as_ref().and_then(|path| {
                let registry = world.resource::<AppTypeRegistry>().clone();
                let registry = registry.read();
                let registration = registry.get_with_type_path(path)?;
                let reflect_component =
                    registration.data::<bevy::ecs::reflect::ReflectComponent>()?;
                let entities: Vec<Entity> = world.iter_entities().map(|e| e.id()).collect();
                entities.into_iter().find_map(|entity| {
                    let entity_ref = world.get_entity(entity).ok()?;
                    reflect_component.reflect(entity_ref)?;
                    world.get::<SceneId>(entity).copied()
                })
            });
            match (type_path, target) {
                (Some(path), Some(id)) => {
                    let leaked: &'static str = Box::leak(path.into_boxed_str());
                    let mut timeline = world.resource_mut::<editor_scene::anim::Timeline>();
                    timeline.track_mut(id, leaked, "bloom").set_key(0.0, 0.0);
                    timeline.track_mut(id, leaked, "bloom").set_key(2.0, 0.8);
                    timeline.generation += 1;
                }
                _ => check(world, false, "the level authors a post-process look"),
            }
        }
        2300 => world.resource_mut::<editor_scene::anim::Playhead>().time = 1.0,
        2310 => {
            let bloom = game_number(world, "PostProcess", "bloom").unwrap_or(0.0);
            check(
                world,
                (bloom - 0.4).abs() < 0.05,
                &format!("a keyframed EFFECT scrubs like anything else ({bloom})"),
            );
        }
        2316 => {
            // And it reaches the render path: intent becomes a real bloom pass.
            let intensity = world
                .query::<&bevy::post_process::bloom::Bloom>()
                .iter(world)
                .map(|bloom| bloom.intensity)
                .fold(0.0_f32, f32::max);
            check(
                world,
                intensity > 0.3,
                &format!("and the camera is actually blooming ({intensity})"),
            );
            shot(world, "48-blockout-effect");
        }
        // ── Easing: the difference between moving and being animated ──────
        // Linear motion reads as machinery. What matters is not that the data
        // changed but that the MOTION did, so this checks the sampled midpoint.
        // Sample a QUARTER of the way in, not the middle: the first ease in the
        // cycle is in-out, which is symmetric, so the midpoint is exactly where
        // it was and comparing there proves nothing. (It read as a failure once,
        // and the failure was the assertion's.)
        2318 => {
            world.resource_mut::<editor_scene::anim::Playhead>().time = 0.5;
            let linear = game_number(world, "PostProcess", "bloom").unwrap_or(0.0);
            world.resource_mut::<BlockoutProbe>().linear_midpoint = linear;
            // Cycle the key at 0.0, which is the one the segment leaves.
            world.resource_mut::<editor_scene::anim::Playhead>().time = 0.0;
        }
        2322 => invoke(world, "anim.ease"),
        2326 => {
            let eased = world
                .resource::<editor_scene::anim::Timeline>()
                .tracks
                .iter()
                .flat_map(|track| track.keys.iter())
                .any(|key| key.ease != editor_scene::anim::Ease::Linear);
            check(world, eased, "cycling changed how the key leaves");
            world.resource_mut::<editor_scene::anim::Playhead>().time = 0.5;
        }
        2332 => {
            let linear = world.resource::<BlockoutProbe>().linear_midpoint;
            let now = game_number(world, "PostProcess", "bloom").unwrap_or(0.0);
            check(
                world,
                (now - linear).abs() > 1e-4,
                &format!("and the MOTION changed with it ({linear} then {now})"),
            );
            check(
                world,
                now < linear,
                &format!("an eased start LAGS a linear one ({now} behind {linear})"),
            );
        }
        2340 => {
            // And still arrives: an ease that misses its own endpoint is a bug
            // with a nice name.
            world.resource_mut::<editor_scene::anim::Playhead>().time = 2.0;
        }
        2348 => {
            let arrived = game_number(world, "PostProcess", "bloom").unwrap_or(0.0);
            check(
                world,
                (arrived - 0.8).abs() < 0.01,
                &format!("and still arrives exactly on the key ({arrived})"),
            );
        }
        // ── An event that makes something HAPPEN ──────────────────────────
        // The whole chain, end to end: a moment marked in the editor, fired by
        // the playhead, translated into a game-side cue, and answered by an
        // emitter the level authored.
        2350 => {
            let count = particle_count(world);
            check(
                world,
                count == 0,
                &format!("nothing in flight yet ({count})"),
            );
            world
                .resource_mut::<editor_scene::anim::Timeline>()
                .events
                .push(editor_scene::anim::EventMarker {
                    time: 0.1,
                    name: "burst".into(),
                });
            world.resource_mut::<editor_scene::anim::Playhead>().time = 0.0;
        }
        2354 => invoke(world, "anim.play"),
        2380 => {
            let count = particle_count(world);
            check(
                world,
                count > 0,
                &format!("the marked moment threw particles ({count})"),
            );
            shot(world, "49-blockout-burst");
            invoke(world, "anim.rewind");
        }
        // And they clean up after themselves: an effect that leaks entities is
        // a memory leak with a pretty face.
        2500 => {
            let count = particle_count(world);
            check(
                world,
                count == 0,
                &format!("and every particle expired ({count})"),
            );
        }
        // ── A trigger volume: the level makes something HAPPEN ─────────────
        // Spec §9 (data-driven gameplay authoring). Everything above proves the
        // editor can author how a level LOOKS and how it MOVES. This proves a
        // designer can place a box that NOTICES — and, just as importantly,
        // that it stays quiet while they are still editing.
        2600 => {
            let count = particle_count(world);
            check(world, count == 0, &format!("nothing in flight ({count})"));
            let stopped = !world.resource::<editor_scene::anim::Playhead>().playing;
            check(
                world,
                stopped,
                "and the playhead is parked, so nothing else can fire",
            );
            // The check that makes every later negative mean something: without
            // it, "no trigger fired" and "the type was never registered" are
            // the same observation.
            let registered = game_type_path(world, "::TriggerVolume").is_some();
            check(
                world,
                registered,
                "the game registered a trigger volume type",
            );
        }
        // Find the actor the way the editor finds anything game-side: by a name
        // in the type registry. `Player` is not reflected, and "find whatever
        // the game marked" is both the contract and the assertion.
        2605 => {
            let Some(actor) = game_marker(world, "::TriggerActor") else {
                check(
                    world,
                    false,
                    "the shipped game marks something as a trigger actor",
                );
                return;
            };
            check(
                world,
                true,
                "the shipped game marks something as a trigger actor",
            );
            let feet = world
                .get::<GlobalTransform>(actor)
                .map(|global| global.translation());
            // The marker rides a child at the feet; what MOVES is its parent.
            let body = world.get::<ChildOf>(actor).map(|parent| parent.parent());
            let mut probe = world.resource_mut::<BlockoutProbe>();
            probe.actor_home = feet;
            probe.actor_body = body;
        }
        // Close the track view this probe opened at 1830: placement aims at the
        // clear strip low in the frame, which is exactly where the panel sits.
        2602 => invoke(world, "timeline.toggle"),
        // Nothing selected: `i` WITH a selection is add-component, not place.
        2606 => invoke(world, "select.clear"),
        2610 => tap(world, KeyCode::KeyI, "i"),
        2620 => type_word(world, "trigg"),
        2630 => tap_named(world, KeyCode::Enter, Key::Enter),
        // Picking a kind arms insert mode; the CLICK is what places. Park the
        // cursor on the clear strip first, the way every other placement in
        // these probes does.
        2640 => {
            let center = crate::probe_user::viewport_center(world);
            move_cursor(world, center);
        }
        2650 => click(world, true),
        2654 => click(world, false),
        2700 => {
            let placed = game_entity(world, "::TriggerVolume").is_some();
            check(world, placed, "a trigger volume placed from the palette");
        }
        // Author it the way a designer would: move it somewhere clear, size it
        // (its size IS its scale), give it a cue of its own, make it one-shot.
        // Every one of these is a patch addressed by FIELD NAME on a game type
        // the editor has never heard of.
        2710 => {
            let Some((entity, id)) = game_entity(world, "::TriggerVolume") else {
                return;
            };
            let Some(volume_path) = game_type_path(world, "::TriggerVolume") else {
                return;
            };
            let Some(burst_path) = game_type_path(world, "::Burst") else {
                return;
            };
            // Where the palette put it — on the clear strip the cursor was
            // parked on, which is also the one place we know is on screen for
            // the click test below. Moving it somewhere "tidy" is how a probe
            // ends up clicking at a negative screen coordinate.
            let Some(placed) = world.get::<Transform>(entity).copied() else {
                return;
            };
            world.resource_mut::<BlockoutProbe>().volume_at = Some(placed.translation);
            let transform = placed.with_scale(Vec3::splat(3.5));
            use bevy::reflect::PartialReflect;
            world.resource_mut::<EditQueue>().0.push(Transaction {
                label: "probe: author the volume".into(),
                gesture: None,
                ops: vec![
                    Op::Set {
                        target: id,
                        value: Box::new(transform).into_partial_reflect(),
                    },
                    Op::Patch {
                        target: id,
                        type_path: volume_path.clone(),
                        path: "name".into(),
                        value: Box::new("gate".to_string()),
                    },
                    Op::Patch {
                        target: id,
                        type_path: volume_path,
                        path: "once".into(),
                        value: Box::new(true),
                    },
                    Op::Patch {
                        target: id,
                        type_path: burst_path,
                        path: "event".into(),
                        value: Box::new("gate".to_string()),
                    },
                ],
            });
        }
        2740 => {
            let named = game_string(world, "::TriggerVolume", "name");
            check(
                world,
                named.as_deref() == Some("gate"),
                &format!("a game field patched by NAME reached the component ({named:?})"),
            );
            let once = game_flag(world, "::TriggerVolume", "once");
            check(world, once == Some(true), "and so did the one-shot flag");
        }
        // A volume has no mesh. The only reason it can be clicked at all is the
        // pick proxy its gizmo registration asks for — and for a thing whose
        // size is authored, that proxy has to BE the box.
        2760 => {
            let Some((entity, _)) = game_entity(world, "::TriggerVolume") else {
                return;
            };
            let proxy = world
                .get::<Children>(entity)
                .map(|children| {
                    children
                        .iter()
                        .filter(|child| {
                            world
                                .get::<crate::feature_gizmos::GizmoPickProxy>(*child)
                                .is_some()
                        })
                        .count()
                })
                .unwrap_or(0);
            check(
                world,
                proxy == 1,
                &format!("an invisible click target ({proxy})"),
            );
        }
        // Placement leaves the new object selected, so clicking it would prove
        // nothing. Drop the selection first and let the click earn it.
        2765 => invoke(world, "select.clear"),
        2770 => {
            let Some(at) = world.resource::<BlockoutProbe>().volume_at else {
                return;
            };
            if let Some(screen) = screen_position_of(world, at) {
                move_cursor(world, screen);
            }
        }
        2775 => click(world, true),
        2779 => click(world, false),
        2800 => {
            let Some((entity, _)) = game_entity(world, "::TriggerVolume") else {
                return;
            };
            let selected = world.get::<Selected>(entity).is_some();
            check(
                world,
                selected,
                "a thing with no geometry is still clickable",
            );
            shot(world, "50-blockout-trigger");
        }
        // Editing is not play. Dragging the player through a checkpoint while
        // authoring must not trip it.
        2810 => pose_actor(world, true),
        2850 => {
            let count = particle_count(world);
            check(
                world,
                count == 0,
                &format!("standing in a volume while EDITING fires nothing ({count})"),
            );
        }
        2860 => pose_actor(world, false),
        2880 => invoke(world, "editor.play"),
        2920 => {
            let playing = !world.resource::<EditorState>().active;
            check(world, playing, "the world goes to the game");
            let count = particle_count(world);
            check(
                world,
                count == 0,
                &format!("and going live with the actor OUTSIDE is quiet ({count})"),
            );
        }
        2930 => pose_actor(world, true),
        // THE assertion. It is the only line that crosses the whole chain: the
        // game marked an actor, the overlay handed the world over, the trigger
        // system ran after propagation, the box contained the feet, the cue
        // went out by name, and the level answered it with an effect.
        2970 => {
            let count = particle_count(world);
            check(
                world,
                count > 0,
                &format!("walking into it throws the effect it names ({count})"),
            );
            shot(world, "51-blockout-trigger-fired");
        }
        2990 => pose_actor(world, false),
        3080 => {
            let count = particle_count(world);
            check(world, count == 0, &format!("the effect expires ({count})"));
            let spent = game_flag(world, "::TriggerState", "spent");
            check(
                world,
                spent == Some(true),
                &format!("and a one-shot that was entered and left is spent ({spent:?})"),
            );
        }
        3090 => pose_actor(world, true),
        3130 => {
            let count = particle_count(world);
            check(
                world,
                count == 0,
                &format!("walking back in does nothing: once means once ({count})"),
            );
        }
        3140 => invoke(world, "editor.reset"),
        3200 => {
            let back = world.resource::<EditorState>().active;
            check(world, back, "reset returns the world to the editor");
            let named = game_string(world, "::TriggerVolume", "name");
            let once = game_flag(world, "::TriggerVolume", "once");
            check(
                world,
                named.as_deref() == Some("gate") && once == Some(true),
                &format!("the authored volume survived the round trip ({named:?}, {once:?})"),
            );
            let spent = game_flag(world, "::TriggerState", "spent");
            check(
                world,
                spent == Some(false),
                &format!("and reset re-armed it, so the next take is a fresh one ({spent:?})"),
            );
        }
        3230 => {
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
