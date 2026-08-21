//! Array — turn one placed piece into a run of N (spec §9 assisted layout).
//!
//! The blockout multiplier. Its whole value is the STEP: copies are spaced by
//! the selection's own extent along the axis, so a wall arrays flush against
//! itself and a run comes out as a wall rather than a dotted line. A fixed grid
//! step cannot do that — a 0.98 m wall on a 1 m grid leaves a visible seam at
//! every joint, and the grid is exactly what a designer is trying not to think
//! about.
//!
//! It is a BAKE, not a modifier: the copies are ordinary entities in one
//! transaction, individually editable afterwards, and one undo removes the run.
//! A live, re-derivable array wants an authoring component and a preview that
//! provably spawns nothing — see the ledger entry; this is the version that can
//! be right today.
//!
//! It lives here rather than in the kernel because it needs three things the
//! kernel cannot see: geometric bounds, the count prompt, and `Socket` by name
//! (a socket's gizmo is a real mesh cone, and counting it would inflate the
//! step of every kit piece).

use crate::PrefabStamped;
use bevy::prelude::*;
use editor_core::layout::{AXIS_NAMES, refusal, skipped_note, subjects};
use editor_core::prelude::*;
use editor_scene::SceneIoFeedback;

/// The same cap painting uses: one instruction should not be able to stall the
/// editor, and a run that quietly lays half of what was asked is worse than one
/// that explains itself.
pub const MAX_ARRAY_COPIES: usize = crate::paint::MAX_PIECES_PER_SEGMENT;

fn say(world: &mut World, message: String, success: bool) {
    world.write_message(SceneIoFeedback { message, success });
}

/// World-space AABB of a subtree, EXCLUDING socket subtrees.
///
/// A socket's gizmo is a real `Mesh3d` cone child, so it carries an `Aabb` and
/// would inflate the step of every kit piece — the run would come out with a
/// gap the width of a gizmo nobody can see in the game.
pub fn world_bounds_no_sockets(world: &World, root: Entity) -> Option<(Vec3, Vec3)> {
    let mut min = Vec3::MAX;
    let mut max = Vec3::MIN;
    let mut stack = vec![root];
    let mut found = false;
    while let Some(entity) = stack.pop() {
        if entity != root && world.get::<crate::sockets::Socket>(entity).is_some() {
            continue; // a socket is not part of the shape
        }
        if let (Some(aabb), Some(global)) = (
            world.get::<bevy::camera::primitives::Aabb>(entity).copied(),
            world.get::<GlobalTransform>(entity).copied(),
        ) {
            let centre = Vec3::from(aabb.center);
            let half = Vec3::from(aabb.half_extents);
            for corner in 0..8 {
                let sign = Vec3::new(
                    if corner & 1 == 0 { -1.0 } else { 1.0 },
                    if corner & 2 == 0 { -1.0 } else { 1.0 },
                    if corner & 4 == 0 { -1.0 } else { 1.0 },
                );
                let point = global.transform_point(centre + half * sign);
                min = min.min(point);
                max = max.max(point);
                found = true;
            }
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    found.then_some((min, max))
}

/// Can this root be copied by value without losing anything?
///
/// `Op::Spawn` carries components and no parentage, so a copy is ONE entity.
/// That is lossless only when everything below the root regenerates: a prefab
/// instance's members are re-stamped, an import's gltf subtree is re-derived
/// and carries no `SceneId` at all. Any other `SceneId`-bearing descendant is
/// real scene content a copy would silently drop — a flattened model's mesh
/// nodes, a loose piece's generated sockets, a hand-built parent/child pair.
///
/// So array REFUSES those, loudly, instead of laying twenty husks somewhere the
/// designer will not look until much later. Copying a subtree properly needs
/// one inverse op per spawned entity, which the edit engine cannot express
/// yet — see the deferred note in the spec.
pub fn copy_safe(world: &World, root: Entity) -> bool {
    let mut stack: Vec<Entity> = world
        .get::<Children>(root)
        .map(|c| c.iter().collect())
        .unwrap_or_default();
    while let Some(entity) = stack.pop() {
        if world.get::<SceneId>(entity).is_some() && world.get::<PrefabStamped>(entity).is_none() {
            return false;
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    true
}

/// The step for one array instruction: the subjects' own extent along the axis,
/// falling back to the grid when nothing has geometry (a light, a spawn point,
/// a trigger volume).
pub fn spacing_for(extent: f32, grid_step: f32) -> f32 {
    if extent > 1e-4 { extent } else { grid_step }
}

pub(crate) fn perform_array(world: &mut World, axis: usize, count: i32) {
    let wanted = count.unsigned_abs() as usize;
    if wanted > MAX_ARRAY_COPIES {
        say(
            world,
            format!("array is capped at {MAX_ARRAY_COPIES} copies"),
            false,
        );
        return;
    }
    let found = subjects(world);
    if let Some(message) = refusal(&found, "array") {
        say(world, message, false);
        return;
    }
    // Copy-safety is a THIRD refusal reason, checked after locked and hidden so
    // its message only appears when it is the actual problem.
    let (safe, unsafe_count): (Vec<(SceneId, Entity)>, usize) = {
        let mut safe = Vec::new();
        let mut refused = 0usize;
        for (id, entity) in &found.subject {
            if copy_safe(world, *entity) {
                safe.push((*id, *entity));
            } else {
                refused += 1;
            }
        }
        (safe, refused)
    };
    if safe.is_empty() {
        say(
            world,
            format!(
                "cannot array {} object{} with child objects \u{b7} g groups them into a prefab first",
                unsafe_count,
                if unsafe_count == 1 { "" } else { "s" }
            ),
            false,
        );
        return;
    }

    let mut min = Vec3::MAX;
    let mut max = Vec3::MIN;
    let mut measured = false;
    for (_, entity) in &safe {
        if let Some((lo, hi)) = world_bounds_no_sockets(world, *entity) {
            min = min.min(lo);
            max = max.max(hi);
            measured = true;
        }
    }
    let grid_step = world.resource::<EditorSettings>().viewport.grid_step;
    let extent = if measured { max[axis] - min[axis] } else { 0.0 };
    let spacing = spacing_for(extent, grid_step);
    let step = Vec3::AXES[axis] * spacing * (count.signum() as f32);

    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let components = world
        .resource::<editor_core::edits::EditorComponents>()
        .types
        .clone();
    let mut ops: Vec<Op> = Vec::new();
    let mut new_ids: Vec<SceneId> = Vec::new();
    {
        let registry = registry_arc.read();
        for k in 1..=wanted {
            for (_, entity) in &safe {
                // The step is a WORLD offset, applied in the entity's own parent
                // frame — a copy of a parented piece has to land where the
                // arithmetic says, not where the same numbers land at the root.
                let parent_inverse = world
                    .get::<ChildOf>(*entity)
                    .and_then(|c| world.get::<GlobalTransform>(c.parent()))
                    .map(|g| g.affine().inverse())
                    .unwrap_or(bevy::math::Affine3A::IDENTITY);
                let local_step = parent_inverse.transform_vector3(step) * k as f32;
                let values: Vec<Box<dyn bevy::reflect::PartialReflect>> = components
                    .iter()
                    .filter_map(|reg| {
                        let reflect_component = registry
                            .get(reg.type_id)?
                            .data::<bevy::ecs::reflect::ReflectComponent>()?;
                        let entity_ref = world.get_entity(*entity).ok()?;
                        let value = reflect_component.reflect(entity_ref)?;
                        if let Some(transform) =
                            value.as_partial_reflect().try_downcast_ref::<Transform>()
                        {
                            let mut stepped = *transform;
                            stepped.translation += local_step;
                            return Some(Box::new(stepped).into_partial_reflect());
                        }
                        Some(value.to_dynamic())
                    })
                    .collect();
                let new_id = SceneId::random();
                ops.push(Op::Spawn {
                    id: new_id,
                    components: values,
                });
                // `Op::Spawn` always spawns at the ROOT, so a copy of a child
                // has to be re-hung. The index observer fires synchronously
                // inside the op, so the id resolves within this same list.
                if let Some(parent_id) = world
                    .get::<ChildOf>(*entity)
                    .and_then(|c| world.get::<SceneId>(c.parent()))
                    .copied()
                {
                    ops.push(Op::Reparent {
                        target: new_id,
                        parent: Some(parent_id),
                    });
                }
                new_ids.push(new_id);
            }
        }
    }
    if ops.is_empty() {
        say(world, "array needs a count".into(), false);
        return;
    }
    let laid = new_ids.len();
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: format!("Array {laid}"),
        gesture: None,
        ops,
    });
    // No auto-grab: duplicate hands its copies to a move gesture because they
    // land ON their originals and look like nothing happened. An array landed
    // exactly where it was aimed, and a move would immediately un-space it.
    world
        .resource_mut::<editor_core::clipboard::PendingPasteSelect>()
        .0 = new_ids;
    let measured_note = if measured {
        format!("{spacing:.2}m, its own width")
    } else {
        format!("{spacing:.2}m, the grid step \u{b7} nothing to measure")
    };
    let mut message = format!(
        "arrayed {laid} along {} \u{b7} {measured_note}{}",
        AXIS_NAMES[axis],
        skipped_note(&found)
    );
    if unsafe_count > 0 {
        message.push_str(&format!(
            " \u{b7} {unsafe_count} skipped (has child objects)"
        ));
    }
    say(world, message, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The step is the piece's own width, so a run tiles flush. Falling back to
    /// the grid is for things with no geometry at all — a light, a spawn point.
    #[test]
    fn spacing_measures_the_piece_and_falls_back_to_the_grid() {
        assert_eq!(spacing_for(2.5, 1.0), 2.5);
        assert_eq!(
            spacing_for(0.98, 1.0),
            0.98,
            "quantizing would seam the run"
        );
        assert_eq!(spacing_for(0.0, 1.0), 1.0);
        assert_eq!(spacing_for(1e-9, 0.5), 0.5);
    }

    /// The bindings the keymap doc promises. A probe run is a slow way to learn
    /// a verb is unreachable.
    #[test]
    fn the_array_verbs_are_bound_where_the_doc_says() {
        let app = crate::tests::test_app();
        let keymap = app
            .world()
            .resource::<editor_core::keymap_data::ResolvedKeymapData>();
        let normal = editor_api::prelude::ContextId::new_static("normal");
        for (action, spelling) in [
            ("transform.array-x", "space x x"),
            ("transform.array-y", "space x y"),
            ("transform.array-z", "space x z"),
        ] {
            let binding: editor_api::keymap::Binding = spelling.parse().expect("parses");
            let rows = keymap
                .by_context
                .get(&normal)
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let found = rows
                .iter()
                .find(|(b, _)| b.0 == binding.0)
                .map(|(_, id)| id.as_str().to_string());
            assert_eq!(
                found,
                Some(action.to_string()),
                "{action} is not on {spelling}"
            );
        }
    }

    /// A copy is ONE entity, so it is lossless only where everything below the
    /// root regenerates. Real child content would be silently dropped, and
    /// arraying it would drop it N times somewhere nobody looks.
    #[test]
    fn copy_safety_admits_regenerating_children_and_refuses_real_ones() {
        let mut world = World::new();
        let lone = world.spawn(SceneId::random()).id();
        assert!(copy_safe(&world, lone), "a lone root is copyable");

        let instance = world.spawn(SceneId::random()).id();
        world.spawn((SceneId::random(), PrefabStamped, ChildOf(instance)));
        assert!(
            copy_safe(&world, instance),
            "a stamped member is rebuilt by the stamper, not copied"
        );

        let derived = world.spawn(SceneId::random()).id();
        world.spawn(ChildOf(derived)); // a gltf node: no SceneId at all
        assert!(
            copy_safe(&world, derived),
            "a derived subtree carries no SceneId and is re-derived"
        );

        let group = world.spawn(SceneId::random()).id();
        world.spawn((SceneId::random(), ChildOf(group)));
        assert!(
            !copy_safe(&world, group),
            "real child content would be dropped by a single-entity copy"
        );
    }

    /// Nesting: the real content can be a grandchild.
    #[test]
    fn copy_safety_looks_all_the_way_down() {
        let mut world = World::new();
        let root = world.spawn(SceneId::random()).id();
        let member = world
            .spawn((SceneId::random(), PrefabStamped, ChildOf(root)))
            .id();
        world.spawn((SceneId::random(), ChildOf(member)));
        assert!(
            !copy_safe(&world, root),
            "a nested real child slipped past the gate"
        );
    }

    /// End to end through the queue: N copies, stepped, in ONE undo entry.
    ///
    /// The subjects have no meshes, so this also pins the grid fallback —
    /// which is the case a bounds-only implementation would divide by zero on.
    #[test]
    fn an_array_lays_a_stepped_run_in_one_undo_entry() {
        let mut app = crate::tests::test_app();
        let id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id,
                    components: vec![
                        Box::new(Transform::from_xyz(1.0, 0.0, 0.0)).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);

        let depth = app.world().resource::<History>().undo_depth();
        perform_array(app.world_mut(), 0, 3);
        app.update();

        let grid = app.world().resource::<EditorSettings>().viewport.grid_step;
        let mut xs: Vec<f32> = app
            .world_mut()
            .query_filtered::<&Transform, With<SceneId>>()
            .iter(app.world())
            .map(|t| t.translation.x)
            .collect();
        xs.sort_by(f32::total_cmp);
        assert_eq!(xs.len(), 4, "a run of 3 copies plus the source");
        for (k, x) in xs.iter().enumerate() {
            let want = 1.0 + grid * k as f32;
            assert!((x - want).abs() < 1e-4, "copy {k} at {x}, wanted {want}");
        }
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth + 1,
            "a run must be ONE undo entry, not one per copy"
        );

        // And it unwinds as one.
        app.world_mut().resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        let left = app
            .world_mut()
            .query_filtered::<(), With<SceneId>>()
            .iter(app.world())
            .count();
        assert_eq!(left, 1, "undo left husks behind");
    }

    /// A negative count runs the other way — the same magnitude, mirrored.
    #[test]
    fn a_negative_count_runs_the_other_way() {
        let mut app = crate::tests::test_app();
        let id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id,
                    components: vec![Box::new(Transform::IDENTITY).into_partial_reflect()],
                }],
            });
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);
        perform_array(app.world_mut(), 2, -2);
        app.update();

        let zs: Vec<f32> = app
            .world_mut()
            .query_filtered::<&Transform, With<SceneId>>()
            .iter(app.world())
            .map(|t| t.translation.z)
            .collect();
        assert_eq!(zs.len(), 3);
        let grid = app.world().resource::<EditorSettings>().viewport.grid_step;
        let mut sorted = zs.clone();
        sorted.sort_by(f32::total_cmp);
        // Distinct and negative: a zero step would satisfy "not positive".
        assert_eq!(sorted.len(), 3);
        for (k, z) in sorted.iter().rev().enumerate() {
            let want = -grid * k as f32;
            assert!((z - want).abs() < 1e-4, "copy {k} at {z}, wanted {want}");
        }
    }

    /// The cap SAYS so. A run that quietly lays 128 of 500 reads as a bug in
    /// the editor rather than a limit.
    #[test]
    fn the_cap_refuses_out_loud() {
        let mut app = crate::tests::test_app();
        let id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id,
                    components: vec![Box::new(Transform::IDENTITY).into_partial_reflect()],
                }],
            });
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);
        perform_array(app.world_mut(), 0, MAX_ARRAY_COPIES as i32 + 1);
        app.update();
        let count = app
            .world_mut()
            .query_filtered::<(), With<SceneId>>()
            .iter(app.world())
            .count();
        assert_eq!(count, 1, "the cap did not hold");
    }

    /// Array must NOT fold a carried child away, and this pins that.
    ///
    /// The move gesture folds because a delta compounds through propagation.
    /// Array SPAWNS: a parent and a child are two independent things to copy,
    /// and nothing compounds. Folding here would turn "arrays the child, and
    /// says it skipped the parent" into "arrays nothing" — a capability loss
    /// wearing a bugfix's clothes.
    #[test]
    fn arraying_a_parent_and_its_child_still_copies_the_child() {
        let mut app = crate::tests::test_app();
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
            app.world_mut().entity_mut(entity).insert(Selected);
        }

        let before = app
            .world_mut()
            .query_filtered::<(), With<SceneId>>()
            .iter(app.world())
            .count();
        perform_array(app.world_mut(), 0, 2);
        app.update();
        let after = app
            .world_mut()
            .query_filtered::<(), With<SceneId>>()
            .iter(app.world())
            .count();
        // The parent is refused (it has real child content a single-entity copy
        // would drop); the child is copied twice.
        assert_eq!(
            after - before,
            2,
            "array laid {} copies, expected the child twice",
            after - before
        );
    }
}
