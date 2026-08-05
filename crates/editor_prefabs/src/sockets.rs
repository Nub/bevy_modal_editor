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

fn reflect_socket(value: &dyn bevy::reflect::PartialReflect) -> Option<Socket> {
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
