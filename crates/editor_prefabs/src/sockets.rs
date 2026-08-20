//! Sockets (spec §9, M4-D9): typed mating points on prefabs. A socket is an
//! ORDINARY child entity carrying `Socket` — so authoring (insert one while the
//! instance is open, move it with the normal gesture), serialization (captured
//! into the template like any member), undo, and propagation all fall out of
//! machinery that already exists. +Z is the mating direction.

use crate::PrefabDef;
use bevy::prelude::*;

/// A typed mating point. Two sockets mate when their `socket_type` matches:
/// positions coincide, +Z axes face each other.
#[derive(Component, Reflect, Clone, PartialEq, Debug)]
#[reflect(Component, Default)]
pub struct Socket {
    pub name: String,
    /// Only equal types mate ("wall", "door", "pipe-m").
    pub socket_type: String,
}

impl Default for Socket {
    fn default() -> Self {
        Self {
            name: "socket".into(),
            socket_type: "default".into(),
        }
    }
}

pub(crate) fn reflect_socket(value: &dyn bevy::reflect::PartialReflect) -> Option<Socket> {
    let matches = value
        .get_represented_type_info()
        .is_some_and(|i| i.type_path() == <Socket as bevy::reflect::TypePath>::type_path());
    if !matches {
        return None;
    }
    <Socket as bevy::reflect::FromReflect>::from_reflect(value)
}

fn record_transform(components: &[Box<dyn bevy::reflect::PartialReflect>]) -> Option<Transform> {
    components.iter().find_map(|c| {
        let value = c.as_partial_reflect();
        let matches = value
            .get_represented_type_info()
            .is_some_and(|i| i.type_path() == <Transform as bevy::reflect::TypePath>::type_path());
        if !matches {
            return None;
        }
        <Transform as bevy::reflect::FromReflect>::from_reflect(value)
    })
}

/// Root-relative socket frames declared by a template, wherever they sit in it.
///
/// It used to take TOP-LEVEL records only. Sockets authored the editor's own
/// way — `space s 2` on a piece, then group — are CHILDREN of that piece, so a
/// prefab built entirely inside this editor reported zero sockets and could
/// never be mated to. Walking the tree and composing each ancestor's transform
/// is also what makes the frames genuinely root-relative rather than
/// accidentally right for flat templates.
pub fn template_sockets(def: &PrefabDef) -> Vec<(Transform, Socket)> {
    let records: Vec<_> = def.template.records().collect();
    let mut sockets = Vec::new();
    for (id, _, components) in &records {
        let Some(socket) = components
            .iter()
            .find_map(|c| reflect_socket(c.as_partial_reflect()))
        else {
            continue;
        };
        // Compose up to the root: a socket three levels down is still declared
        // relative to the thing you place.
        let mut transform = record_transform(components).unwrap_or_default();
        let mut current = *id;
        let mut guard = 0;
        while let Some((_, Some(parent), _)) =
            records.iter().find(|(other, _, _)| *other == current)
        {
            let Some((_, _, parent_components)) =
                records.iter().find(|(other, _, _)| other == parent)
            else {
                break;
            };
            let parent_transform = record_transform(parent_components).unwrap_or_default();
            transform = parent_transform.mul_transform(transform);
            current = *parent;
            guard += 1;
            if guard > 64 {
                break; // a malformed template must not hang an import
            }
        }
        sockets.push((transform, socket));
    }
    sockets
}

/// The root transform that mates `local` (a socket frame relative to the root)
/// with `target` (a socket frame in world space): positions coincide, +Z axes
/// face each other. root = target × flipY(180°) × local⁻¹.
pub fn mate_transform(target: &GlobalTransform, local: &Transform) -> Transform {
    let flip = Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::PI));
    let target = target.compute_transform();
    let inverse = Transform::from_matrix(local.to_matrix().inverse());
    let combined = target.to_matrix() * flip.to_matrix() * inverse.to_matrix();
    Transform::from_matrix(combined)
}

/// Two sockets are CONNECTED when they are in the same place, facing each
/// other, and of the same type.
///
/// Nothing records a mate: mating computes a transform and sets it, and the
/// pieces are then simply adjacent. So connection is read back off the
/// geometry, which has the useful property that it stays true when a designer
/// achieves the same joint by hand.
///
/// The tolerance is deliberately tight — this answers "are these two joined",
/// not "are these two nearby", and the mate math places sockets exactly.
/// The keymap layer that is live while a socket is armed. A modal editor
/// answers "does this break `i`?" by rebinding `i` — in socket mode it means
/// "place a piece HERE" rather than "add a component to what I hold".
pub const SOCKET_CONTEXT: editor_api::prelude::ContextId =
    editor_api::prelude::ContextId::new_static("socket");

pub const JOINT_TOLERANCE: f32 = 0.02;

pub fn sockets_are_joined(a: &GlobalTransform, b: &GlobalTransform) -> bool {
    if a.translation().distance(b.translation()) > JOINT_TOLERANCE {
        return false;
    }
    // Mated sockets oppose: +Z into +Z. Anything else is two sockets that
    // happen to share a spot, which is not a joint.
    (a.forward().dot(*b.forward()) + 1.0).abs() < 0.05
}
/// One way the moved piece could mate: where its root must go, and which pair
/// of sockets would meet.
#[derive(Clone, Debug)]
pub struct MateCandidate {
    /// Where the moved root has to be for the two sockets to coincide.
    pub root: Transform,
    /// The socket on the world side.
    pub target: Entity,
    /// Gap between the two sockets before the move, in metres.
    pub distance: f32,
    pub label: String,
}

/// The best mate available, or nothing.
///
/// **Measures SOCKET TO SOCKET.** It used to measure from the moved piece's
/// ORIGIN to the target socket, which quietly made mating impossible for
/// anything big: a six-metre wall carries its sockets three metres from its
/// own origin, so bringing the origin within reach of a socket means burying
/// the wall inside its neighbour. Everything about the workflow looked correct
/// and nothing ever snapped.
///
/// Pure, so the preview and the commit cannot disagree about what will happen —
/// they call this with the same arguments and get the same answer.
pub fn best_mate(
    candidates: &[(Entity, GlobalTransform, Socket)],
    mating: &[(Transform, Socket)],
    root_now: &GlobalTransform,
    radius: f32,
) -> Option<MateCandidate> {
    let mut best: Option<MateCandidate> = None;
    for (target_entity, target_global, target_socket) in candidates {
        for (local, socket) in mating {
            // Types are the kit's compatibility rule: a pipe does not mate to
            // a wall however close it is.
            if socket.socket_type != target_socket.socket_type {
                continue;
            }
            // Where THIS socket is right now, in the world.
            let source_world = root_now.mul_transform(*local).translation();
            let distance = source_world.distance(target_global.translation());
            if distance > radius {
                continue;
            }
            if best.as_ref().is_none_or(|found| distance < found.distance) {
                best = Some(MateCandidate {
                    root: mate_transform(target_global, local),
                    target: *target_entity,
                    distance,
                    label: format!("snapped {} \u{2194} {}", socket.name, target_socket.name),
                });
            }
        }
    }
    best
}

/// Every socket in the world that could be mated TO, minus the ones belonging
/// to the piece being moved (a piece must never snap to itself).
pub fn mate_candidates(
    world: &mut World,
    exclude: &[Entity],
) -> Vec<(Entity, GlobalTransform, Socket)> {
    world
        .query::<(Entity, &GlobalTransform, &Socket)>()
        .iter(world)
        .filter(|(entity, _, _)| !exclude.contains(entity))
        .map(|(entity, global, socket)| (entity, *global, socket.clone()))
        .collect()
}

/// Best snap for PLACING a piece whose root would land at `at`.
///
/// A thin wrapper over [`best_mate`] so placement and drag-commit cannot
/// disagree about what mates with what — the preview promising a snap the
/// commit then declines to make is the exact regression D9 was written for.
pub fn snap_for_placement(
    world: &mut World,
    def_sockets: &[(Transform, Socket)],
    at: Vec3,
    radius: f32,
) -> Option<(Transform, String)> {
    if def_sockets.is_empty() {
        return None;
    }
    let candidates = mate_candidates(world, &[]);
    let root_now = GlobalTransform::from(Transform::from_translation(at));
    best_mate(&candidates, def_sockets, &root_now, radius).map(|found| (found.root, found.label))
}

/// The socket the designer has SELECTED, if any.
pub fn selected_socket(world: &mut World) -> Option<(Entity, GlobalTransform, Socket)> {
    world
        .query_filtered::<(Entity, &GlobalTransform, &Socket), With<editor_core::selection::Selected>>()
        .iter(world)
        .next()
        .map(|(entity, global, socket)| (entity, *global, socket.clone()))
}

/// Where a piece should land — THE placement decision, in one place so every
/// path agrees.
///
/// A SELECTED SOCKET wins over the cursor. That is the "select a socket and
/// spawn the next object there" loop: pick the end of the run, pick the piece,
/// and it arrives mated, without hunting for a hover position that is within
/// reach of the right socket. Falls back to the best mate near the cursor, and
/// then to the cursor itself.
///
/// A piece with no sockets still honours a selected socket by landing AT it —
/// putting a barrel where you pointed is more useful than ignoring what you
/// said because the barrel has no mating points.
pub fn placement_for(
    world: &mut World,
    def_sockets: &[(Transform, Socket)],
    at: Vec3,
    radius: f32,
) -> (Transform, Option<String>) {
    if let Some((entity, target, target_socket)) = selected_socket(world) {
        let mating: Vec<(Transform, Socket)> = def_sockets
            .iter()
            .filter(|(_, socket)| socket.socket_type == target_socket.socket_type)
            .cloned()
            .collect();
        if !mating.is_empty() {
            let candidates = vec![(entity, target, target_socket.clone())];
            // Reach is irrelevant here: the designer NAMED the socket, so the
            // piece comes to it however far away the cursor happens to be.
            if let Some(found) = best_mate(
                &candidates,
                &mating,
                &GlobalTransform::from(Transform::from_translation(target.translation())),
                f32::MAX,
            ) {
                return (found.root, Some(found.label));
            }
        }
        return (
            Transform::from_translation(target.translation()),
            Some(format!("placed at {}", target_socket.name)),
        );
    }
    match snap_for_placement(world, def_sockets, at, radius) {
        Some((transform, label)) => (transform, Some(label)),
        None => (Transform::from_translation(at), None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A joint is two sockets in the same place FACING each other. Two sockets
    /// that merely share a spot — a piece crossing another — are not joined,
    /// and rotating about them would swing the wrong thing.
    #[test]
    fn a_joint_is_coincident_and_opposed() {
        let a = GlobalTransform::from(Transform::from_xyz(2.0, 0.0, 0.0));
        // Facing back at it: +Z opposed.
        let b = GlobalTransform::from(
            Transform::from_xyz(2.0, 0.0, 0.0)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
        );
        assert!(sockets_are_joined(&a, &b), "mated");

        // Same spot, same direction — crossing, not joined.
        let parallel = GlobalTransform::from(Transform::from_xyz(2.0, 0.0, 0.0));
        assert!(!sockets_are_joined(&a, &parallel));

        // Opposed but apart.
        let apart = GlobalTransform::from(
            Transform::from_xyz(2.5, 0.0, 0.0)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
        );
        assert!(!sockets_are_joined(&a, &apart));
    }

    /// What `mate_transform` produces must READ BACK as a joint, or "pivot on
    /// the connected socket" can never find the joint the editor just made.
    #[test]
    fn a_mate_produces_a_joint() {
        let target = GlobalTransform::from(
            Transform::from_xyz(3.0, 1.0, -2.0).with_rotation(Quat::from_euler(
                EulerRot::XYZ,
                0.2,
                0.9,
                -0.3,
            )),
        );
        let local = Transform::from_xyz(1.5, 0.0, 0.0);
        let root = mate_transform(&target, &local);
        // Where the moved piece's socket ends up.
        let source = GlobalTransform::from(root).mul_transform(local);
        assert!(
            sockets_are_joined(&source, &target),
            "the editor's own mate reads back as a joint"
        );
    }

    /// THE bug behind "I cannot seem to snap objects to sockets": reach was
    /// measured from the moved piece's ORIGIN to the target socket. A six-metre
    /// wall carries its sockets three metres out, so its origin only comes
    /// within reach once the wall is buried in its neighbour — mating was
    /// arithmetically impossible for exactly the pieces kits are made of.
    #[test]
    fn a_big_piece_mates_by_its_socket_not_its_origin() {
        let target = GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 0.0));
        let candidates = vec![(Entity::from_raw_u32(1).unwrap(), target, Socket::default())];
        // A 6m wall: origin in the middle, sockets 3m out either side.
        let mating = vec![
            (Transform::from_xyz(-3.0, 0.0, 0.0), Socket::default()),
            (Transform::from_xyz(3.0, 0.0, 0.0), Socket::default()),
        ];
        // Dragged so its LEFT socket is 20cm from the target — visually touching.
        let root_now = GlobalTransform::from(Transform::from_xyz(3.2, 0.0, 0.0));
        let found = best_mate(&candidates, &mating, &root_now, 1.0)
            .expect("a socket 0.2m away is in reach");
        assert!((found.distance - 0.2).abs() < 1e-5, "{}", found.distance);
        // And the origin is 3.2m away, which no sane reach would admit — the
        // measure has to be the socket's.
        assert!(root_now.translation().distance(target.translation()) > 3.0);
    }

    /// Reach is reach: a socket further away than the radius does not mate,
    /// however big the piece is.
    #[test]
    fn out_of_reach_is_out_of_reach() {
        let target = GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 0.0));
        let candidates = vec![(Entity::from_raw_u32(1).unwrap(), target, Socket::default())];
        let mating = vec![(Transform::from_xyz(-3.0, 0.0, 0.0), Socket::default())];
        let root_now = GlobalTransform::from(Transform::from_xyz(6.0, 0.0, 0.0));
        assert!(best_mate(&candidates, &mating, &root_now, 1.0).is_none());
    }

    /// Types are the kit's compatibility rule: a pipe does not mate to a wall
    /// however close it is.
    #[test]
    fn types_must_match() {
        let target = GlobalTransform::from(Transform::default());
        let candidates = vec![(
            Entity::from_raw_u32(1).unwrap(),
            target,
            Socket {
                name: "a".into(),
                socket_type: "wall".into(),
            },
        )];
        let mating = vec![(
            Transform::default(),
            Socket {
                name: "b".into(),
                socket_type: "pipe".into(),
            },
        )];
        assert!(best_mate(&candidates, &mating, &GlobalTransform::default(), 5.0).is_none());
    }

    /// The nearest pair wins, not the first one found.
    #[test]
    fn the_nearest_pair_wins() {
        let near = Entity::from_raw_u32(1).unwrap();
        let far = Entity::from_raw_u32(2).unwrap();
        let candidates = vec![
            (
                far,
                GlobalTransform::from(Transform::from_xyz(0.9, 0.0, 0.0)),
                Socket::default(),
            ),
            (
                near,
                GlobalTransform::from(Transform::from_xyz(0.1, 0.0, 0.0)),
                Socket::default(),
            ),
        ];
        let mating = vec![(Transform::default(), Socket::default())];
        let found = best_mate(&candidates, &mating, &GlobalTransform::default(), 2.0).unwrap();
        assert_eq!(found.target, near);
    }

    /// A socket authored the editor's own way — generated onto a piece, then
    /// grouped — is a CHILD record. Reading only root-level records reported
    /// zero sockets, so a prefab built entirely in this editor could not be
    /// mated to at all.
    #[test]
    fn nested_sockets_are_found_and_root_relative() {
        let piece = editor_api::prelude::SceneId::random();
        let socket_id = editor_api::prelude::SceneId::random();
        let template = editor_scene::snapshot_from_parts(vec![
            (
                piece,
                None,
                vec![Box::new(Transform::from_xyz(0.0, 1.0, 0.0)).into_partial_reflect()],
            ),
            (
                socket_id,
                Some(piece),
                vec![
                    Box::new(Transform::from_xyz(2.0, 0.0, 0.0)).into_partial_reflect(),
                    Box::new(Socket::default()).into_partial_reflect(),
                ],
            ),
        ]);
        let def = PrefabDef {
            kit: None,
            id: uuid::Uuid::new_v4(),
            name: "Wall".into(),
            template,
        };
        let sockets = template_sockets(&def);
        assert_eq!(sockets.len(), 1, "the nested socket is found");
        // Composed through its parent: 2 along x from a piece lifted 1 in y.
        assert_eq!(sockets[0].0.translation, Vec3::new(2.0, 1.0, 0.0));
    }

    // D9 math pin: the mated root places its socket EXACTLY on the target,
    // +Z axes opposed.
    #[test]
    fn mating_is_exact() {
        // Target socket somewhere in the world, rotated arbitrarily.
        let target = GlobalTransform::from(
            Transform::from_xyz(4.0, 1.0, -2.0).with_rotation(Quat::from_euler(
                EulerRot::XYZ,
                0.3,
                1.2,
                -0.4,
            )),
        );
        // The placed prefab's socket, offset + rotated relative to its root.
        let local = Transform::from_xyz(0.5, 0.0, 1.0).with_rotation(Quat::from_rotation_y(0.7));

        let root = mate_transform(&target, &local);
        let socket_world = root.to_matrix() * local.to_matrix();
        let socket_world = Transform::from_matrix(socket_world);

        let target_t = target.compute_transform();
        assert!(
            socket_world.translation.distance(target_t.translation) < 1e-4,
            "positions coincide: {:?} vs {:?}",
            socket_world.translation,
            target_t.translation
        );
        let placed_z = socket_world.rotation * Vec3::Z;
        let target_z = target_t.rotation * Vec3::Z;
        assert!(
            placed_z.dot(target_z) < -0.999,
            "+Z axes face each other: {placed_z:?} vs {target_z:?}"
        );
    }
}

/// D10 kit coherence: within a kit, every socket TYPE needs a counterpart —
/// a type appearing on only one piece can never mate; a kit piece with no
/// sockets can never join. Warnings, not errors (kits grow incrementally).
pub fn kit_coherence(library: &crate::PrefabLibrary) -> Vec<String> {
    use std::collections::HashMap;
    let mut kits: HashMap<&str, Vec<&crate::PrefabDef>> = HashMap::new();
    for def in library.prefabs.values() {
        if let Some(kit) = &def.kit {
            kits.entry(kit.as_str()).or_default().push(def);
        }
    }
    let mut warnings = Vec::new();
    for (kit, defs) in kits {
        let mut type_owners: HashMap<String, Vec<&str>> = HashMap::new();
        for def in &defs {
            let sockets = template_sockets(def);
            if sockets.is_empty() {
                warnings.push(format!("kit {kit}: {} has no sockets", def.name));
            }
            for (_, socket) in sockets {
                type_owners
                    .entry(socket.socket_type)
                    .or_default()
                    .push(def.name.as_str());
            }
        }
        for (socket_type, owners) in type_owners {
            if owners.len() < 2 {
                warnings.push(format!(
                    "kit {kit}: socket type \"{socket_type}\" only on {} — nothing mates with it",
                    owners.first().copied().unwrap_or("?")
                ));
            }
        }
    }
    warnings.sort();
    warnings
}

#[cfg(test)]
mod coherence_tests {
    use super::*;
    use crate::{PrefabDef, PrefabLibrary};
    use uuid::Uuid;

    fn piece(name: &str, kit: &str, socket_types: &[&str]) -> PrefabDef {
        PrefabDef {
            kit: Some(kit.into()),
            id: Uuid::new_v4(),
            name: name.into(),
            template: editor_scene::snapshot_from_parts(
                socket_types
                    .iter()
                    .map(|t| {
                        (
                            editor_api::prelude::SceneId::random(),
                            None,
                            vec![
                                Box::new(Socket {
                                    name: "s".into(),
                                    socket_type: (*t).into(),
                                })
                                .into_partial_reflect(),
                                Box::new(Transform::default()).into_partial_reflect(),
                            ],
                        )
                    })
                    .collect(),
            ),
        }
    }

    #[test]
    fn coherence_flags_loners_and_socketless() {
        let mut library = PrefabLibrary::default();
        for def in [
            piece("Wall", "walls", &["wall", "wall"]),
            piece("Corner", "walls", &["wall", "roof"]), // "roof" has no counterpart
            piece("Rock", "walls", &[]),                 // socketless kit member
        ] {
            library.prefabs.insert(def.id, def);
        }
        let warnings = kit_coherence(&library);
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("\"roof\" only on Corner")),
            "loner socket type flagged: {warnings:?}"
        );
        assert!(
            warnings.iter().any(|w| w.contains("Rock has no sockets")),
            "socketless member flagged: {warnings:?}"
        );
        assert_eq!(warnings.len(), 2, "wall type is coherent: {warnings:?}");
    }
}

// ---------------------------------------------------------------------------
// Placement helpers (owner ask): a socket only mates if it sits exactly on the
// surface AND aims out of it. Both are fiddly to type by hand and impossible to
// eyeball, so snap to the geometry the piece actually has: put the socket where
// you roughly want it, then snap it to the nearest face, edge, or corner.
// ---------------------------------------------------------------------------

/// Which feature of the bounding box a socket snaps to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SnapFeature {
    /// Centre of the nearest face; +Z points straight out of it.
    Face,
    /// Midpoint of the nearest edge; +Z bisects the two faces meeting there.
    Edge,
    /// Nearest corner; +Z bisects the three faces meeting there.
    Corner,
}

/// Snap `point` (in the piece's local space) onto `bounds`, returning the
/// placed transform: position on the feature, +Z along its outward normal.
///
/// `min`/`max` are the piece's local bounds. Each feature is chosen by
/// quantizing the point's offset from the centre — the axis it is furthest
/// along picks a face, the two furthest pick an edge, all three a corner. That
/// makes the choice continuous with where the user already dragged the socket.
pub fn snap_to_bounds(point: Vec3, min: Vec3, max: Vec3, feature: SnapFeature) -> Transform {
    let centre = (min + max) * 0.5;
    let half = ((max - min) * 0.5).max(Vec3::splat(1e-4));
    // Normalized offset per axis: ±1 at the faces, 0 at the centre.
    let normalized = (point - centre) / half;
    let ranked = {
        // Axes ordered by how far out the point sits — the feature picks the
        // top 1 (face), 2 (edge) or 3 (corner).
        let mut axes = [0usize, 1, 2];
        axes.sort_by(|a, b| {
            normalized[*b]
                .abs()
                .partial_cmp(&normalized[*a].abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        axes
    };
    let used = match feature {
        SnapFeature::Face => 1,
        SnapFeature::Edge => 2,
        SnapFeature::Corner => 3,
    };
    let mut position = point.clamp(min, max);
    let mut normal = Vec3::ZERO;
    for &axis in ranked.iter().take(used) {
        // A point exactly on the centre plane has no side; pick the positive
        // one so the result is deterministic rather than NaN-signed.
        let side = if normalized[axis] < 0.0 { -1.0 } else { 1.0 };
        position[axis] = centre[axis] + side * half[axis];
        normal[axis] = side;
    }
    let normal = normal.normalize_or(Vec3::Z);
    Transform::from_translation(position).with_rotation(Quat::from_rotation_arc(Vec3::Z, normal))
}

#[cfg(test)]
mod snap_tests {
    use super::*;

    // A 2×2×2 box centred on the origin: the features are at ±1.
    const MIN: Vec3 = Vec3::splat(-1.0);
    const MAX: Vec3 = Vec3::splat(1.0);

    fn mating_direction(t: &Transform) -> Vec3 {
        t.rotation * Vec3::Z
    }

    #[test]
    fn face_snaps_to_the_nearest_side_and_aims_out() {
        // Nearest to +X, so the socket lands mid-face aiming +X.
        let placed = snap_to_bounds(Vec3::new(0.9, 0.2, -0.1), MIN, MAX, SnapFeature::Face);
        assert_eq!(placed.translation.x, 1.0, "sits ON the face");
        assert!(
            mating_direction(&placed).abs_diff_eq(Vec3::X, 1e-5),
            "aims out of it: {:?}",
            mating_direction(&placed)
        );
        // The other axes keep the authored position — sliding along the face.
        assert!((placed.translation.y - 0.2).abs() < 1e-5);
    }

    #[test]
    fn edge_uses_two_axes_and_bisects_them() {
        let placed = snap_to_bounds(Vec3::new(0.9, -0.8, 0.1), MIN, MAX, SnapFeature::Edge);
        assert_eq!((placed.translation.x, placed.translation.y), (1.0, -1.0));
        assert!(
            mating_direction(&placed).abs_diff_eq(Vec3::new(1.0, -1.0, 0.0).normalize(), 1e-5),
            "bisects the two faces: {:?}",
            mating_direction(&placed)
        );
    }

    #[test]
    fn corner_pins_all_three_axes() {
        let placed = snap_to_bounds(Vec3::new(-0.7, 0.6, 0.5), MIN, MAX, SnapFeature::Corner);
        assert_eq!(placed.translation, Vec3::new(-1.0, 1.0, 1.0));
        assert!(
            mating_direction(&placed).abs_diff_eq(Vec3::new(-1.0, 1.0, 1.0).normalize(), 1e-5),
            "bisects all three"
        );
    }

    // Off-centre, non-cubic bounds: the snap must use the ACTUAL box, not an
    // origin-centred guess (imported models rarely sit on their origin).
    #[test]
    fn respects_offset_and_non_uniform_bounds() {
        let min = Vec3::new(0.0, 0.0, -0.5);
        let max = Vec3::new(4.0, 1.0, 0.5);
        let placed = snap_to_bounds(Vec3::new(3.9, 0.5, 0.0), min, max, SnapFeature::Face);
        assert_eq!(placed.translation.x, 4.0, "lands on the far face");
        assert!(mating_direction(&placed).abs_diff_eq(Vec3::X, 1e-5));
    }

    // A degenerate (flat) box must not produce NaNs.
    #[test]
    fn flat_bounds_stay_finite() {
        let placed = snap_to_bounds(Vec3::ZERO, Vec3::ZERO, Vec3::ZERO, SnapFeature::Corner);
        assert!(placed.translation.is_finite());
        assert!(mating_direction(&placed).is_finite());
    }
}

/// Which faces `socket.generate-*` puts a socket on. Named for the LAYOUT they
/// produce, because that is what you are actually choosing: a run of walls, a
/// grid of tiles, or a stack.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SocketSides {
    /// ±X only — a piece that chains end to end (a wall run).
    Ends,
    /// ±X and ±Z — a piece that tiles in the plane (a floor grid).
    Sides,
    /// All six faces, ±Y included (shafts, stacked blocks).
    All,
}

impl SocketSides {
    /// Outward face normals, with the name each socket gets.
    pub fn faces(self) -> &'static [(&'static str, Vec3)] {
        const ENDS: &[(&str, Vec3)] = &[("+X", Vec3::X), ("-X", Vec3::NEG_X)];
        const SIDES: &[(&str, Vec3)] = &[
            ("+X", Vec3::X),
            ("-X", Vec3::NEG_X),
            ("+Z", Vec3::Z),
            ("-Z", Vec3::NEG_Z),
        ];
        const ALL: &[(&str, Vec3)] = &[
            ("+X", Vec3::X),
            ("-X", Vec3::NEG_X),
            ("+Z", Vec3::Z),
            ("-Z", Vec3::NEG_Z),
            ("+Y", Vec3::Y),
            ("-Y", Vec3::NEG_Y),
        ];
        match self {
            Self::Ends => ENDS,
            Self::Sides => SIDES,
            Self::All => ALL,
        }
    }
}

/// The socket frame centred on the face of `min..max` whose outward normal is
/// `normal`: on the surface, +Z pointing out.
pub fn face_socket(min: Vec3, max: Vec3, normal: Vec3) -> Transform {
    let centre = (min + max) * 0.5;
    let half = (max - min) * 0.5;
    let position = centre + normal * half;
    Transform::from_translation(position).with_rotation(Quat::from_rotation_arc(Vec3::Z, normal))
}
