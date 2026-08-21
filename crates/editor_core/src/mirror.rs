//! Mirror the selection across an axis-aligned plane (spec §9 assisted layout).
//!
//! A plane reflection is IMPROPER — its determinant is −1 — and no `Transform`
//! can hold one without a negative scale, which flips winding, breaks lighting
//! and confuses physics. This editor does not do that. It reflects the
//! PLACEMENT exactly and CONJUGATES the orientation: `R·M·R` has determinant +1
//! for every M, so it is a real rotation, and it is the exact answer whenever
//! the piece is symmetric about the plane's direction — which every wall,
//! floor, pillar and crate in a blockout is.
//!
//! What it does not do is flip CHIRALITY: an L-corner comes out rotated, not
//! reflected. Bounds cannot detect that (an L has symmetric bounds), so rather
//! than guess, the feedback says "placement only, geometry is not flipped"
//! every single time. A tool that quietly does the wrong thing to one piece in
//! twenty is worse than one that tells you its limits.

use crate::layout::{AXIS_NAMES, refusal, skipped_note, transform_subjects};

use bevy::math::Affine3A;
use bevy::prelude::*;
use editor_api::prelude::*;

/// Reflect a world pose across the plane through `origin` with unit normal `n`.
///
/// Scale is returned untouched, on purpose: see the module note.
pub fn mirror_world(
    scale: Vec3,
    rotation: Quat,
    at: Vec3,
    n: Vec3,
    origin: Vec3,
) -> (Vec3, Quat, Vec3) {
    let n = n.normalize();
    // Householder reflection: I − 2nnᵀ. Its own inverse, determinant −1.
    let outer = Mat3::from_cols(n * n.x, n * n.y, n * n.z);
    let reflect = Mat3::IDENTITY - 2.0 * outer;
    // Conjugation, not composition: R·M·R is proper, so it is a real rotation.
    let conjugated = reflect * Mat3::from_quat(rotation) * reflect;
    let rotation = Quat::from_mat3(&conjugated).normalize();
    let translation = at - 2.0 * (at - origin).dot(n) * n;
    (scale, rotation, translation)
}

#[derive(Resource, Default)]
pub(crate) struct MirrorRequests {
    plane: Option<usize>,
}

pub(crate) fn collect_mirror_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<crate::resolver::EditorState>,
    mut requests: ResMut<MirrorRequests>,
) {
    if !state.active {
        return;
    }
    for invoked in reader.read() {
        let plane = match invoked.action.as_str() {
            "transform.mirror-x" => 0,
            "transform.mirror-y" => 1,
            "transform.mirror-z" => 2,
            _ => continue,
        };
        requests.plane = Some(plane);
    }
}

fn say(world: &mut World, message: String, success: bool) {
    world.write_message(editor_api::feedback::SceneIoFeedback { message, success });
}

pub(crate) fn perform_mirror(world: &mut World) {
    let Some(plane) = world.resource_mut::<MirrorRequests>().plane.take() else {
        return;
    };
    if !world.resource::<crate::resolver::EditorState>().active {
        return;
    }
    let normal = Vec3::AXES[plane];
    let found = transform_subjects(world);
    if let Some(message) = refusal(&found, "mirror") {
        say(world, message, false);
        return;
    }

    // The plane passes through the selection's own centre, so mirroring a pair
    // swaps them and mirroring one leaves it put — which is what "mirror the
    // selection" means when nothing else has been nominated as an origin.
    let poses: Vec<(SceneId, Entity, GlobalTransform)> = found
        .subject
        .iter()
        .filter_map(|(id, entity)| {
            world
                .get::<GlobalTransform>(*entity)
                .map(|g| (*id, *entity, *g))
        })
        .collect();
    if poses.is_empty() {
        say(world, "select something to mirror".into(), false);
        return;
    }
    let origin = poses.iter().map(|(_, _, g)| g.translation()).sum::<Vec3>() / poses.len() as f32;

    let mut ops: Vec<Op> = Vec::new();
    for (id, entity, global) in &poses {
        let (scale, rotation, at) = global.to_scale_rotation_translation();
        let (scale, rotation, at) = mirror_world(scale, rotation, at, normal, origin);
        let mirrored = GlobalTransform::from(Affine3A::from_scale_rotation_translation(
            scale, rotation, at,
        ));
        // Written back in the entity's OWN parent frame: a mirrored child of a
        // group must land where the world arithmetic says, not where the same
        // numbers would land at the root.
        let parent = world
            .get::<ChildOf>(*entity)
            .and_then(|c| world.get::<GlobalTransform>(c.parent()))
            .copied()
            .unwrap_or(GlobalTransform::IDENTITY);
        ops.push(Op::Set {
            target: *id,
            value: Box::new(mirrored.reparented_to(&parent)).into_partial_reflect(),
        });
    }
    let count = ops.len();
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: format!("Mirror {}", AXIS_NAMES[plane]),
        gesture: None,
        ops,
    });
    say(
        world,
        format!(
            "mirrored {count} across {} \u{b7} placement only, geometry is not flipped{}",
            AXIS_NAMES[plane],
            skipped_note(&found)
        ),
        true,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const X: Vec3 = Vec3::X;

    #[test]
    fn a_point_reflects_across_the_plane() {
        let (_, _, at) = mirror_world(
            Vec3::ONE,
            Quat::IDENTITY,
            Vec3::new(5.0, 1.0, 2.0),
            X,
            Vec3::ZERO,
        );
        assert!((at - Vec3::new(-5.0, 1.0, 2.0)).length() < 1e-5);
    }

    /// The plane rides the selection, so a lone object mirrors onto itself.
    #[test]
    fn mirroring_about_its_own_centre_leaves_a_point_put() {
        let at = Vec3::new(3.0, 0.0, -4.0);
        let (_, _, out) = mirror_world(Vec3::ONE, Quat::IDENTITY, at, X, at);
        assert!((out - at).length() < 1e-5);
    }

    /// The case a naive "flip the axis nearest the normal" gets wrong: a wall
    /// facing −X at x=+5 must come out at x=−5 facing +X — back into the
    /// corridor, not away from it.
    #[test]
    fn a_wall_mirrors_to_face_back_across_the_plane() {
        let facing = Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2);
        let (_, rotation, at) =
            mirror_world(Vec3::ONE, facing, Vec3::new(5.0, 0.0, 0.0), X, Vec3::ZERO);
        assert!((at.x + 5.0).abs() < 1e-5);
        let forward = rotation * Vec3::Z;
        let before = facing * Vec3::Z;
        assert!(
            forward.x * before.x < 0.0,
            "the mirrored wall faces the same way it did: {before} -> {forward}"
        );
    }

    /// Yaw negates across an axis plane. The oracle for X: (x, −y, −z, w).
    #[test]
    fn yaw_negates_across_the_x_plane() {
        let yaw = Quat::from_rotation_y(0.7);
        let (_, rotation, _) = mirror_world(Vec3::ONE, yaw, Vec3::ZERO, X, Vec3::ZERO);
        let expected = Quat::from_rotation_y(-0.7);
        assert!(
            rotation.dot(expected).abs() > 0.9999,
            "{rotation:?} is not {expected:?}"
        );
    }

    /// THE invariant. A negative scale flips winding, breaks lighting and
    /// confuses physics; the reflection is absorbed into the rotation instead.
    #[test]
    fn scale_never_goes_negative() {
        for angle in [0.0, 0.3, 1.1, 2.9] {
            for normal in [Vec3::X, Vec3::Y, Vec3::Z] {
                let rotation = Quat::from_euler(EulerRot::XYZ, angle, angle * 0.5, angle * 0.25);
                let (scale, out, _) = mirror_world(
                    Vec3::new(1.0, 2.0, 3.0),
                    rotation,
                    Vec3::ONE,
                    normal,
                    Vec3::ZERO,
                );
                assert_eq!(scale, Vec3::new(1.0, 2.0, 3.0), "scale was touched");
                assert!(
                    Mat3::from_quat(out).determinant() > 0.0,
                    "the orientation came out improper"
                );
            }
        }
    }

    /// Mirroring twice is the identity — the reflection is its own inverse.
    #[test]
    fn mirroring_twice_returns_the_original() {
        let rotation = Quat::from_euler(EulerRot::XYZ, 0.4, -1.2, 0.9);
        let at = Vec3::new(2.0, -3.0, 4.0);
        let origin = Vec3::new(1.0, 0.0, 0.0);
        let (s1, r1, t1) = mirror_world(Vec3::ONE, rotation, at, Vec3::Z, origin);
        let (_, r2, t2) = mirror_world(s1, r1, t1, Vec3::Z, origin);
        assert!((t2 - at).length() < 1e-5);
        assert!(r2.dot(rotation).abs() > 0.9999);
    }

    /// Mirror writes a POSE onto an existing transform, computed from the
    /// pre-mirror parent frame — so a parent and its selected child would each
    /// reflect, and the parent's own op would then invalidate the frame the
    /// child's was built in. The parent alone is the operand.
    #[test]
    fn mirroring_a_parent_and_its_child_mirrors_the_parent_alone() {
        let mut app = App::new();
        app.add_plugins(crate::EditorCorePlugin);
        app.init_resource::<bevy::input::ButtonInput<KeyCode>>();
        app.finish();
        app.update();
        app.world_mut()
            .resource_mut::<crate::resolver::EditorState>()
            .active = true;

        let (parent, child) = (SceneId::random(), SceneId::random());
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![
                    Op::Spawn {
                        id: parent,
                        components: vec![Box::new(Transform::default()).into_partial_reflect()],
                    },
                    Op::Spawn {
                        id: child,
                        components: vec![
                            Box::new(Transform::from_xyz(1.0, 0.0, 0.0)).into_partial_reflect(),
                        ],
                    },
                    Op::Reparent {
                        target: child,
                        parent: Some(parent),
                    },
                ],
            });
        app.update();
        for id in [parent, child] {
            let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
            app.world_mut()
                .entity_mut(entity)
                .insert(crate::selection::Selected);
        }

        app.world_mut().resource_mut::<MirrorRequests>().plane = Some(0);
        perform_mirror(app.world_mut());
        let queued = &app.world().resource::<EditQueue>().0;
        let ops: Vec<&Op> = queued.iter().flat_map(|t| t.ops.iter()).collect();
        assert_eq!(ops.len(), 1, "the carried child was mirrored too");
        assert!(
            matches!(ops[0], Op::Set { target, .. } if *target == parent),
            "the surviving operand was not the parent"
        );
    }
}
