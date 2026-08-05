//! Wall painting (spec §9 "architectural painting", M4-D10 exit): select a
//! socketed piece, invoke Paint, then click a polyline on the ground — pieces
//! chain along every segment through the SAME mating math as `o`/snap, corner
//! pieces from the kit resolve direction changes, and each segment commits as
//! ONE undoable transaction. Esc leaves paint mode.

use crate::sockets::{mate_transform, template_sockets};
use crate::{PrefabDef, PrefabInstance, PrefabLibrary, PrefabOverrides};
use bevy::prelude::*;
use editor_core::prelude::*;
use uuid::Uuid;

pub const PAINT_CONTEXT: ContextId = ContextId::new_static("paint");

#[derive(Resource, Default)]
pub struct PaintState(pub Option<PaintData>);

pub struct PaintData {
    pub prefab: Uuid,
    /// Polyline anchor: None until the first click.
    pub start: Option<Vec3>,
    /// Exit frame of the last laid piece — the next segment chains from it.
    pub cursor_exit: Option<GlobalTransform>,
    /// Direction of the previous segment (corner detection).
    pub previous_direction: Option<Vec3>,
}

/// Entry/exit socket frames of a def (repeat-piece convention: entry first,
/// exit the farthest socket from it).
fn entry_exit(def: &PrefabDef) -> Option<(Transform, Transform)> {
    let sockets = template_sockets(def);
    let (entry, _) = sockets.first()?;
    let exit = sockets
        .iter()
        .max_by(|(a, _), (b, _)| {
            a.translation
                .distance(entry.translation)
                .total_cmp(&b.translation.distance(entry.translation))
        })
        .map(|(t, _)| *t)?;
    Some((*entry, exit))
}

/// The kit's corner piece: a DIFFERENT def in the same kit whose two mating
/// directions sit at roughly a right angle (geometry, not naming).
fn corner_def<'a>(library: &'a PrefabLibrary, piece: &PrefabDef) -> Option<&'a PrefabDef> {
    let kit = piece.kit.as_ref()?;
    library.prefabs.values().find(|candidate| {
        if candidate.id == piece.id || candidate.kit.as_ref() != Some(kit) {
            return false;
        }
        let sockets = template_sockets(candidate);
        let [(a, _), (b, _)] = sockets.as_slice() else {
            return false;
        };
        let dot = (a.rotation * Vec3::Z).dot(b.rotation * Vec3::Z).abs();
        dot < 0.35 // ~70–110° apart = a corner
    })
}

/// Plan one painted segment: pieces chained from `from` (an exit frame, or a
/// synthesized frame at `start` facing back along the stroke) toward `end`.
/// Pure math — pinned by test. Returns the spawn transforms + the segment's
/// final exit frame.
pub fn plan_segment(
    def: &PrefabDef,
    from: Option<GlobalTransform>,
    start: Vec3,
    end: Vec3,
) -> Option<(Vec<Transform>, GlobalTransform)> {
    let (entry, exit) = entry_exit(def)?;
    let piece_length = entry.translation.distance(exit.translation);
    if piece_length < 1e-3 {
        return None;
    }
    let stroke = end - start;
    let length = stroke.length();
    if length < piece_length * 0.5 {
        return None;
    }
    let direction = stroke.normalize();
    // Chain anchor: the previous exit frame, or a virtual EXIT at `start`
    // pointing along the stroke — mating flips the entry against it, so the
    // piece body extends forward.
    let mut anchor = from.unwrap_or_else(|| {
        // Anchor at SOCKET height: clicks land on the ground, sockets sit at
        // the piece's mating height — otherwise every root sinks by that much.
        GlobalTransform::from(
            Transform::from_translation(start + Vec3::Y * entry.translation.y)
                .with_rotation(Quat::from_rotation_arc(Vec3::Z, direction)),
        )
    });
    let count = ((length / piece_length).round() as usize).max(1);
    let mut placements = Vec::with_capacity(count);
    for _ in 0..count {
        let root = mate_transform(&anchor, &entry);
        anchor = GlobalTransform::from(root) * GlobalTransform::from(exit).compute_transform();
        placements.push(root);
    }
    Some((placements, anchor))
}

pub(crate) fn collect_paint_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<EditorState>,
    mut requests: ResMut<PaintRequests>,
) {
    if !state.active {
        return;
    }
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "prefab.paint" => requests.enter = true,
            "paint.exit" => requests.exit = true,
            _ => {}
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct PaintRequests {
    enter: bool,
    exit: bool,
}

pub(crate) fn perform_paint_actions(world: &mut World) {
    let requests = std::mem::take(&mut *world.resource_mut::<PaintRequests>());
    if requests.enter {
        enter_paint(world);
    }
    if requests.exit && world.resource::<PaintState>().0.is_some() {
        world.resource_mut::<PaintState>().0 = None;
        world.resource_mut::<OverlayContext>().0 = None;
        world.write_message(editor_scene::SceneIoFeedback {
            message: "paint done".into(),
            success: true,
        });
    }
}

fn enter_paint(world: &mut World) {
    let Some(root_id) = crate::authoring::selected_instance_roots(world)
        .first()
        .copied()
    else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select a socketed piece to paint with".into(),
            success: false,
        });
        return;
    };
    let Some(root) = world.resource::<SceneIndex>().get(&root_id) else {
        return;
    };
    let Some(instance) = world.get::<PrefabInstance>(root).copied() else {
        return;
    };
    let (name, has_sockets) = {
        let library = world.resource::<PrefabLibrary>();
        let Some(def) = library.prefabs.get(&instance.0) else {
            return;
        };
        (def.name.clone(), entry_exit(def).is_some())
    };
    if !has_sockets {
        world.write_message(editor_scene::SceneIoFeedback {
            message: format!("{name} needs two sockets to paint with"),
            success: false,
        });
        return;
    }
    world.resource_mut::<PaintState>().0 = Some(PaintData {
        prefab: instance.0,
        start: None,
        cursor_exit: None,
        previous_direction: None,
    });
    world.resource_mut::<OverlayContext>().0 = Some(PAINT_CONTEXT);
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("painting {name} — click to anchor, click to lay, ⎋ done"),
        success: true,
    });
}

/// Clicks while painting: first anchors the polyline, each further click lays
/// a segment (ONE transaction) and continues from its end.
pub(crate) fn paint_click(world: &mut World) {
    if world.resource::<PaintState>().0.is_none() {
        return;
    }
    let clicked = world
        .get_resource::<ButtonInput<MouseButton>>()
        .is_some_and(|m| m.just_pressed(MouseButton::Left));
    if !clicked
        || world
            .resource::<editor_core::resolver::PointerOverChrome>()
            .0
    {
        return;
    }
    let Some(point) = world.resource::<CursorGround>().0 else {
        return;
    };
    let (prefab, start, cursor_exit, previous_direction) = {
        let state = world.resource::<PaintState>();
        let data = state.0.as_ref().unwrap();
        (
            data.prefab,
            data.start,
            data.cursor_exit,
            data.previous_direction,
        )
    };
    let Some(start) = start else {
        if let Some(data) = world.resource_mut::<PaintState>().0.as_mut() {
            data.start = Some(point);
        }
        return;
    };

    let (def, corner) = {
        let library = world.resource::<PrefabLibrary>();
        let Some(def) = library.prefabs.get(&prefab) else {
            return;
        };
        let corner = corner_def(library, def).map(|c| (c.id, entry_exit(c), c.name.clone()));
        (
            PrefabDef {
                kit: def.kit.clone(),
                id: def.id,
                name: def.name.clone(),
                template: editor_scene::snapshot_from_parts(
                    def.template
                        .records()
                        .map(|(id, parent, c)| {
                            (id, parent, c.iter().map(|v| v.to_dynamic()).collect())
                        })
                        .collect(),
                ),
            },
            corner,
        )
    };

    let direction = (point - start).normalize_or_zero();
    let mut ops = Vec::new();
    let mut anchor = cursor_exit;
    let mut corner_name = None;

    // Direction change beyond ~15°: lay the kit's corner at the joint first.
    if let (Some(previous), Some(exit_frame)) = (previous_direction, anchor)
        && previous.dot(direction) < 0.966
        && let Some((corner_id, Some((corner_entry, corner_exit)), name)) = corner
    {
        let corner_root = mate_transform(&exit_frame, &corner_entry);
        anchor = Some(
            GlobalTransform::from(corner_root)
                * GlobalTransform::from(corner_exit).compute_transform(),
        );
        ops.push(Op::Spawn {
            id: SceneId::random(),
            components: vec![
                Box::new(PrefabInstance(corner_id)).into_partial_reflect(),
                Box::new(PrefabOverrides::default()).into_partial_reflect(),
                Box::new(corner_root).into_partial_reflect(),
                Box::new(Name::new(name.clone())).into_partial_reflect(),
            ],
        });
        corner_name = Some(name);
    }

    let Some((placements, exit_frame)) = plan_segment(&def, anchor, start, point) else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "stroke too short for a piece".into(),
            success: false,
        });
        return;
    };
    let laid = placements.len();
    for root in placements {
        ops.push(Op::Spawn {
            id: SceneId::random(),
            components: vec![
                Box::new(PrefabInstance(def.id)).into_partial_reflect(),
                Box::new(PrefabOverrides::default()).into_partial_reflect(),
                Box::new(root).into_partial_reflect(),
                Box::new(Name::new(def.name.clone())).into_partial_reflect(),
            ],
        });
    }
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: format!("Paint {}", def.name),
        gesture: None,
        ops,
    });
    if let Some(data) = world.resource_mut::<PaintState>().0.as_mut() {
        data.start = Some(point);
        data.cursor_exit = Some(exit_frame);
        data.previous_direction = Some(direction);
    }
    world.write_message(editor_scene::SceneIoFeedback {
        message: match corner_name {
            Some(corner) => format!("laid {laid} × {} + {corner} — ⎋ done", def.name),
            None => format!("laid {laid} × {} — click on, ⎋ done", def.name),
        },
        success: true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sockets::Socket;

    fn wall_def() -> PrefabDef {
        let socket = |x: f32, dir: Vec3| {
            (
                SceneId::random(),
                None,
                vec![
                    Box::new(Socket {
                        name: "s".into(),
                        socket_type: "wall".into(),
                    })
                    .into_partial_reflect(),
                    Box::new(
                        Transform::from_xyz(x, 0.5, 0.0)
                            .with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
                    )
                    .into_partial_reflect(),
                ],
            )
        };
        PrefabDef {
            kit: Some("walls".into()),
            id: Uuid::new_v4(),
            name: "Wall".into(),
            template: editor_scene::snapshot_from_parts(vec![
                socket(-1.0, -Vec3::X),
                socket(1.0, Vec3::X),
            ]),
        }
    }

    // D10 exit math: a 6m stroke lays 3 walls end-to-end along it, and the
    // segment's exit lands at the stroke end (chaining is gap-free).
    #[test]
    fn segment_planning_is_exact() {
        let def = wall_def();
        let start = Vec3::new(2.0, 0.0, 3.0);
        let end = Vec3::new(8.0, 0.0, 3.0);
        let (placements, exit) = plan_segment(&def, None, start, end).unwrap();
        assert_eq!(placements.len(), 3, "6m / 2m walls = 3 pieces");
        let mut xs: Vec<f32> = placements.iter().map(|t| t.translation.x).collect();
        xs.sort_by(f32::total_cmp);
        assert!(
            (xs[0] - 3.0).abs() < 1e-3 && (xs[1] - 5.0).abs() < 1e-3 && (xs[2] - 7.0).abs() < 1e-3,
            "wall centers at 3/5/7 along the stroke: {xs:?}"
        );
        assert!(
            exit.translation().distance(Vec3::new(8.0, 0.5, 3.0)) < 1e-3,
            "segment exit at the stroke end: {:?}",
            exit.translation()
        );
    }
}
