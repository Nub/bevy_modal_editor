//! Socket authoring probe (SOCKET_PROBE=1): the flow the owner actually walks
//! — place a piece, put a socket on it, snap the socket to the geometry — in
//! the REAL binary. Socket authoring has broken twice without a probe noticing;
//! this is the coverage that would have caught it.

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use editor_core::prelude::*;
use editor_prefabs::sockets::Socket;

use crate::probe_user::{shot, tap_named};

#[derive(Resource, Default)]
pub(crate) struct SocketProbe {
    frame: u32,
    failures: Vec<String>,
    piece: Option<SceneId>,
    socket: Option<SceneId>,
    loose: Option<SceneId>,
    generated: usize,
}

fn check(world: &mut World, ok: bool, what: &str) {
    if ok {
        info!("SOCKET-PROBE PASS: {what}");
    } else {
        error!("SOCKET-PROBE FAIL: {what}");
        world
            .resource_mut::<SocketProbe>()
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

fn socket_transform(world: &mut World, id: SceneId) -> Option<Transform> {
    let entity = world.resource::<editor_api::edits::SceneIndex>().get(&id)?;
    world.get::<Transform>(entity).copied()
}

fn select_only(world: &mut World, id: SceneId) {
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
}

pub(crate) fn probe_socket(world: &mut World) {
    use bevy::reflect::PartialReflect;
    use editor_api::edits::{EditQueue, Op, Transaction};

    world.resource_mut::<SocketProbe>().frame += 1;
    let frame = world.resource::<SocketProbe>().frame;
    if frame == 1 {
        let _ = std::fs::create_dir_all(crate::probe_user::SHOT_DIR);
        info!("SOCKET-PROBE armed");
    }
    match frame {
        60 => tap_named(world, KeyCode::Enter, Key::Enter),
        120 => invoke(world, "core.toggle-editor"),
        // A piece with real geometry, and a socket parented to it — the shape
        // the adopt pass produces when you insert inside an open instance.
        200 => {
            let piece = SceneId::random();
            let socket = SceneId::random();
            let loose = SceneId::random();
            {
                let mut probe = world.resource_mut::<SocketProbe>();
                probe.piece = Some(piece);
                probe.socket = Some(socket);
                probe.loose = Some(loose);
            }
            world.resource_mut::<EditQueue>().0.push(Transaction {
                label: "probe piece".into(),
                gesture: None,
                ops: vec![
                    Op::Spawn {
                        id: piece,
                        components: vec![
                            Box::new(Transform::from_xyz(4.0, 1.0, 4.0)).into_partial_reflect(),
                            Box::new(Name::new("probe piece")).into_partial_reflect(),
                        ],
                    },
                    Op::Spawn {
                        id: socket,
                        components: vec![
                            Box::new(Socket {
                                name: "east".into(),
                                socket_type: "wall".into(),
                            })
                            .into_partial_reflect(),
                            // Floating NEAR the +X side, not on it.
                            Box::new(Transform::from_xyz(0.35, 0.1, 0.05)).into_partial_reflect(),
                            Box::new(Name::new("probe socket")).into_partial_reflect(),
                        ],
                    },
                    Op::Reparent {
                        target: socket,
                        parent: Some(piece),
                    },
                    // A socket with NO parent, just off the piece's -X face:
                    // snapping must adopt it and place it there.
                    Op::Spawn {
                        id: loose,
                        components: vec![
                            Box::new(Socket::default()).into_partial_reflect(),
                            Box::new(Transform::from_xyz(3.2, 1.0, 4.0)).into_partial_reflect(),
                            Box::new(Name::new("loose socket")).into_partial_reflect(),
                        ],
                    },
                ],
            });
        }
        // Real geometry on the piece: bounds are what the snap measures, and
        // they arrive as an Aabb from the mesh — derived render state, exactly
        // as a game's regenerate systems would attach it.
        240 => {
            let piece = world.resource::<SocketProbe>().piece.unwrap();
            if let Some(entity) = world
                .resource::<editor_api::edits::SceneIndex>()
                .get(&piece)
            {
                world
                    .entity_mut(entity)
                    .insert(bevy::camera::primitives::Aabb {
                        center: Vec3::ZERO.into(),
                        half_extents: Vec3::splat(0.5).into(),
                    });
            }
        }
        // The gizmo must exist and be VISIBLE — an invisible socket cannot be
        // clicked, which is how "I can't see or move them" starts.
        280 => {
            let socket = world.resource::<SocketProbe>().socket;
            let entity =
                socket.and_then(|id| world.resource::<editor_api::edits::SceneIndex>().get(&id));
            // `ViewVisibility`, not `InheritedVisibility`. The latter goes
            // STALE when propagation never visits the entity — it keeps
            // whatever it last held while nothing renders — and that is
            // exactly the failure this assertion missed: the cone read
            // "inherited: visible" while the viewport showed no cones at all.
            // `ViewVisibility` is recomputed per view per frame, so it is the
            // only one that answers "does it draw".
            // Whether the cone can DRAW, tested as the invariant rather than
            // the symptom. `ViewVisibility` also goes false for anything
            // outside the frustum, so it cannot tell "hidden" from "off
            // camera"; `InheritedVisibility` goes STALE when propagation never
            // visits, which is exactly how a broken chain hides. So: the cone
            // reads visible AND every link from it to the root carries the
            // components propagation walks through.
            let drawable = entity.is_some_and(|socket| {
                let cone_ok = world
                    .get::<Children>(socket)
                    .map(|children| {
                        let kids: Vec<Entity> = children.iter().collect();
                        kids.into_iter().any(|child| {
                            world.get::<Mesh3d>(child).is_some()
                                && world
                                    .get::<InheritedVisibility>(child)
                                    .is_some_and(|v| v.get())
                        })
                    })
                    .unwrap_or(false);
                let mut chain_ok = true;
                let mut current = socket;
                loop {
                    if world.get::<InheritedVisibility>(current).is_none() {
                        chain_ok = false;
                        break;
                    }
                    match world.get::<ChildOf>(current) {
                        Some(parent) => current = parent.parent(),
                        None => break,
                    }
                }
                cone_ok && chain_ok
            });
            check(world, drawable, "the socket draws a visible gizmo");
        }
        320 => {
            let socket = world.resource::<SocketProbe>().socket.unwrap();
            select_only(world, socket);
        }
        340 => invoke(world, "socket.snap-face"),
        380 => {
            let socket = world.resource::<SocketProbe>().socket.unwrap();
            let placed = socket_transform(world, socket);
            // The piece is a 1m cube centred on its origin: +X face is at 0.5.
            check(
                world,
                placed.is_some_and(|t| (t.translation.x - 0.5).abs() < 0.05),
                &format!("snap put the socket ON the +X face ({placed:?})"),
            );
            check(
                world,
                placed.is_some_and(|t| (t.rotation * Vec3::Z).abs_diff_eq(Vec3::X, 0.05)),
                "snap aimed +Z out of the face",
            );
            shot(world, "30-socket-snap");
        }
        // The parentless case: a clear refusal, and the socket unmoved.
        420 => {
            let loose = world.resource::<SocketProbe>().loose.unwrap();
            select_only(world, loose);
        }
        440 => invoke(world, "socket.snap-face"),
        480 => {
            // "Put this socket on that object" is the whole intent: a socket
            // with no parent is ADOPTED by the nearest piece and snapped, so
            // the verb works straight after inserting one.
            let (loose, piece) = {
                let probe = world.resource::<SocketProbe>();
                (probe.loose.unwrap(), probe.piece.unwrap())
            };
            let index = world.resource::<editor_api::edits::SceneIndex>();
            let (loose_entity, piece_entity) = (index.get(&loose), index.get(&piece));
            let adopted = loose_entity.and_then(|e| world.get::<ChildOf>(e).map(|c| c.parent()))
                == piece_entity;
            check(
                world,
                adopted,
                "a parentless socket is adopted by the piece",
            );
            let placed = socket_transform(world, loose);
            check(
                world,
                placed.is_some_and(|t| (t.translation.x + 0.5).abs() < 0.05),
                &format!("and lands on the piece's -X face ({placed:?})"),
            );
        }
        // Generate a mating set: a tile that grids needs four, and a re-run
        // must top up rather than double up.
        520 => {
            let piece = world.resource::<SocketProbe>().piece.unwrap();
            select_only(world, piece);
        }
        540 => invoke(world, "socket.generate-sides"),
        580 => {
            let piece = world.resource::<SocketProbe>().piece.unwrap();
            let entity = world
                .resource::<editor_api::edits::SceneIndex>()
                .get(&piece)
                .unwrap();
            let sockets: Vec<Vec3> = world
                .get::<Children>(entity)
                .map(|children| {
                    children
                        .iter()
                        .filter(|c| world.get::<Socket>(*c).is_some())
                        .filter_map(|c| world.get::<Transform>(c).map(|t| t.translation))
                        .collect()
                })
                .unwrap_or_default();
            // Two were already there (the parented one, snapped to +X, and the
            // adopted loose one on -X), so ±Z are the new ones.
            check(
                world,
                sockets.iter().any(|s| (s.z - 0.5).abs() < 0.01)
                    && sockets.iter().any(|s| (s.z + 0.5).abs() < 0.01),
                &format!("generate put sockets on the ±Z faces ({sockets:?})"),
            );
            world.resource_mut::<SocketProbe>().generated = sockets.len();
        }
        600 => invoke(world, "socket.generate-sides"),
        640 => {
            let (piece, before) = {
                let probe = world.resource::<SocketProbe>();
                (probe.piece.unwrap(), probe.generated)
            };
            let entity = world
                .resource::<editor_api::edits::SceneIndex>()
                .get(&piece)
                .unwrap();
            let now = world
                .get::<Children>(entity)
                .map(|children| {
                    children
                        .iter()
                        .filter(|c| world.get::<Socket>(*c).is_some())
                        .count()
                })
                .unwrap_or(0);
            check(
                world,
                now == before,
                &format!("re-running tops up rather than duplicating ({before} → {now})"),
            );
            shot(world, "31-socket-generate");
        }
        // Sockets must survive a library change: adopting one into a STAMPED
        // subtree used to lose it on the next restamp ("worked for a second,
        // then stopped").
        660 => {
            world
                .resource_mut::<editor_prefabs::PrefabLibrary>()
                .generation += 1;
        }
        700 => {
            let piece = world.resource::<SocketProbe>().piece.unwrap();
            let entity = world
                .resource::<editor_api::edits::SceneIndex>()
                .get(&piece)
                .unwrap();
            let survived = world
                .get::<Children>(entity)
                .map(|children| {
                    children
                        .iter()
                        .filter(|c| world.get::<Socket>(*c).is_some())
                        .count()
                })
                .unwrap_or(0);
            check(
                world,
                survived == world.resource::<SocketProbe>().generated,
                &format!("sockets survive a library restamp ({survived} left)"),
            );
        }
        // ── The level's floor and sun are SCENE entities (owner): in the
        //    hierarchy, selectable, with their data on the inspector ────────
        1150 => {
            // Registered gizmos: the game says how ITS component looks, and the
            // editor gives it a click target (spec §7 designer surface).
            let gizmos = world.resource::<GizmoCatalog>().gizmos.len();
            check(world, gizmos >= 1, "the game registered a custom gizmo");
            let named: Vec<String> = world
                .query_filtered::<&Name, With<SceneId>>()
                .iter(world)
                .map(|n| n.as_str().to_string())
                .collect();
            check(
                world,
                named.iter().any(|n| n == "Ground"),
                &format!("the floor is a scene entity ({named:?})"),
            );
            check(
                world,
                named.iter().any(|n| n == "Sun"),
                "the light is a scene entity",
            );
            // And they carry their DATA, so they survive a save/load round trip
            // rather than coming back as empty husks.
            // Its mesh is DERIVED (the editor cannot name the game's Ground
            // type, but a regenerated mesh is the observable proof).
            let ground_meshed = world
                .query_filtered::<(&Name, Has<Mesh3d>), With<SceneId>>()
                .iter(world)
                .any(|(name, meshed)| name.as_str() == "Ground" && meshed);
            check(world, ground_meshed, "the floor derives its mesh from data");
            let sun_is_data = world
                .query_filtered::<(), (With<SceneId>, With<DirectionalLight>)>()
                .iter(world)
                .count();
            check(world, sun_is_data >= 1, "the light carries its own data");
            let spawn_named = world
                .query_filtered::<&Name, With<SceneId>>()
                .iter(world)
                .any(|n| n.as_str() == "Player Spawn");
            check(world, spawn_named, "the player spawn is a scene entity");
            // Gizmo-only widgets get an invisible pick sphere, or they could be
            // seen and never clicked.
            let pickable = world
                .query_filtered::<(), With<crate::feature_gizmos::GizmoPickProxy>>()
                .iter(world)
                .count();
            check(world, pickable >= 1, "the spawn widget is clickable");
        }
        // ── Orientation widget: it tracks the camera, and clicking an axis
        //    ball takes you to that view (owner ask) ─────────────────────────
        1200 => {
            let balls = world
                .query_filtered::<(), With<crate::view_gizmo::AxisBall>>()
                .iter(world)
                .count();
            check(world, balls == 6, "the orientation widget shows six axes");
            invoke(world, "view.front");
        }
        1240 => {
            // Front view: the +Z ball faces the viewer, so it must sit at the
            // widget's centre — the gizmo is reading the camera, not a guess.
            let centred = world
                .query::<(&crate::view_gizmo::AxisBall, &Node)>()
                .iter(world)
                .find(|(ball, _)| ball.axis().z > 0.5)
                .map(|(_, node)| (node.left, node.top));
            let at_centre = matches!(
                centred,
                Some((bevy::ui::Val::Px(x), bevy::ui::Val::Px(y)))
                    if (x - 34.0).abs() < 2.0 && (y - 34.0).abs() < 2.0
            );
            check(
                world,
                at_centre,
                &format!("the widget tracks the camera ({centred:?})"),
            );
            let ortho = world
                .query::<(&Camera, &Projection)>()
                .iter(world)
                .any(|(_, p)| matches!(p, Projection::Orthographic(_)));
            check(world, ortho, "1 gives an orthographic front view");
            shot(world, "33-view-gizmo");
        }
        1280 => invoke(world, "view.perspective"),
        1320 => {
            let perspective = world
                .query::<(&Camera, &Projection)>()
                .iter(world)
                .any(|(_, p)| matches!(p, Projection::Perspective(_)));
            check(world, perspective, "4 returns to the perspective view");
        }
        // Chaining must keep going: group the socketed piece into a prefab and
        // press `o` three times. Sockets GENERATED onto a placed instance live
        // in the scene, not the template — requiring template membership was
        // why "sockets aren't chaining with more than 1".
        740 => {
            let piece = world.resource::<SocketProbe>().piece.unwrap();
            select_only(world, piece);
            world
                .resource_mut::<editor_prefabs::authoring::GroupCommit>()
                .0 = Some("chainpiece".into());
            world
                .resource_mut::<editor_prefabs::authoring::GroupPrompt>()
                .purpose = editor_prefabs::authoring::PromptPurpose::Group;
        }
        800 | 860 | 920 => invoke(world, "prefab.repeat"),
        980 => {
            let instances: Vec<Vec3> = world
                .query_filtered::<&Transform, (
                    With<editor_prefabs::PrefabInstance>,
                    Without<editor_scene::PrefabStamped>,
                )>()
                .iter(world)
                .map(|t| t.translation)
                .collect();
            check(
                world,
                instances.len() >= 4,
                &format!("o chains repeatedly ({} instances)", instances.len()),
            );
            // A run, not a stack: every piece at a distinct spot.
            let mut distinct = instances.clone();
            distinct.dedup_by(|a, b| a.distance(*b) < 0.01);
            check(
                world,
                distinct.len() == instances.len(),
                &format!("each chained piece lands somewhere new ({instances:?})"),
            );
            shot(world, "32-socket-chain");
        }
        1400 => {
            let failures = world.resource::<SocketProbe>().failures.clone();
            if failures.is_empty() {
                info!("SOCKET-PROBE PASS: socket authoring end-to-end");
                world.write_message(AppExit::Success);
            } else {
                error!("SOCKET-PROBE FAILED: {failures:?}");
                world.write_message(AppExit::error());
            }
        }
        _ => {}
    }
}
