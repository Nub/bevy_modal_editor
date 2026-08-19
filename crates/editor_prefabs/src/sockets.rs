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

/// Root-relative socket frames declared by a template (top-level records only —
/// nested-instance sockets belong to their own prefab).
pub fn template_sockets(def: &PrefabDef) -> Vec<(Transform, Socket)> {
    def.template
        .records()
        .filter(|(_, parent, _)| parent.is_none())
        .filter_map(|(_, _, components)| {
            let socket = components
                .iter()
                .find_map(|c| reflect_socket(c.as_partial_reflect()))?;
            Some((record_transform(components).unwrap_or_default(), socket))
        })
        .collect()
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

/// Best snap for placing `def` near `at`: nearest world socket within `radius`
/// whose type matches one of the template's sockets. Returns the mated root
/// transform and a description for feedback.
pub fn snap_for_placement(
    world: &mut World,
    def_sockets: &[(Transform, Socket)],
    at: Vec3,
    radius: f32,
) -> Option<(Transform, String)> {
    if def_sockets.is_empty() {
        return None;
    }
    let candidates: Vec<(GlobalTransform, Socket)> = {
        let mut query = world.query::<(&GlobalTransform, &Socket)>();
        query.iter(world).map(|(g, s)| (*g, s.clone())).collect()
    };
    let mut best: Option<(f32, Transform, String)> = None;
    for (target_global, target_socket) in &candidates {
        let distance = target_global.translation().distance(at);
        if distance > radius {
            continue;
        }
        for (local, socket) in def_sockets {
            if socket.socket_type != target_socket.socket_type {
                continue;
            }
            if best.as_ref().is_none_or(|(d, _, _)| distance < *d) {
                best = Some((
                    distance,
                    mate_transform(target_global, local),
                    format!("snapped {} \u{2194} {}", socket.name, target_socket.name),
                ));
            }
        }
    }
    best.map(|(_, transform, label)| (transform, label))
}

#[cfg(test)]
mod tests {
    use super::*;

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
