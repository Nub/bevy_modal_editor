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

use bevy::prelude::*;
use editor_core::edits::{CopyRefusal, CopySubtree, EditorComponents, copy_ops, copy_subtree};
use editor_core::layout::{AXIS_NAMES, copy_subjects, refusal, skipped_note};
use editor_core::prelude::*;
use editor_scene::SceneIoFeedback;

/// The same cap painting uses: one instruction should not be able to stall the
/// editor, and a run that quietly lays half of what was asked is worse than one
/// that explains itself.
pub const MAX_ARRAY_COPIES: usize = crate::paint::MAX_PIECES_PER_SEGMENT;

/// And a bound on the total, because a copy is a SUBTREE now: 128 copies of a
/// forty-part group is five thousand spawns in one transaction.
pub const MAX_ARRAY_ENTITIES: usize = 2048;

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
    let found = copy_subjects(world);
    if let Some(message) = refusal(&found, "array") {
        say(world, message, false);
        return;
    }
    // Capture ONCE per subject, then stamp it N times. What cannot be copied
    // is counted so the message can name it: a generated part has no business
    // being copied at all, and a root with real content under a generated
    // member would lose it silently — the one case the old blanket refusal
    // was right about, and all that is left of it.
    let (safe, derived, lossy): (Vec<(SceneId, Entity, CopySubtree)>, usize, usize) = {
        let registry_arc = world.resource::<AppTypeRegistry>().clone();
        let owned = EditorComponents {
            types: world.resource::<EditorComponents>().types.clone(),
        };
        let registry = registry_arc.read();
        let mut safe = Vec::new();
        let (mut derived, mut lossy) = (0usize, 0usize);
        for (id, entity) in &found.subject {
            match copy_subtree(world, &registry, &owned, *entity) {
                Ok(subtree) => safe.push((*id, *entity, subtree)),
                Err(CopyRefusal::DerivedRoot) => derived += 1,
                Err(CopyRefusal::LosesContentUnderDerived) => lossy += 1,
                Err(CopyRefusal::Unnamed) => {}
            }
        }
        drop(registry);
        (safe, derived, lossy)
    };
    if safe.is_empty() {
        let plural = if derived + lossy == 1 { "" } else { "s" };
        let message = if lossy == 0 {
            format!(
                "cannot array {derived} generated part{plural} \u{b7} select the object it belongs to"
            )
        } else {
            format!(
                "cannot array {} object{plural} with parts inside a generated subtree",
                derived + lossy
            )
        };
        say(world, message, false);
        return;
    }
    // The cap bounds ENTITIES, not roots: it was written when a copy was one
    // entity, and 128 copies of a forty-part group is five thousand spawns in
    // one transaction — against the cap's own reason for existing.
    let per_copy: usize = safe.iter().map(|(_, _, s)| s.records.len()).sum();
    if per_copy * wanted > MAX_ARRAY_ENTITIES {
        say(
            world,
            format!(
                "array is capped at {MAX_ARRAY_ENTITIES} entities \u{b7} {wanted} copies of this \
                 would lay {}",
                per_copy * wanted
            ),
            false,
        );
        return;
    }

    let mut min = Vec3::MAX;
    let mut max = Vec3::MIN;
    let mut measured = false;
    for (_, entity, _) in &safe {
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

    let mut ops: Vec<Op> = Vec::new();
    let mut new_roots: Vec<SceneId> = Vec::new();
    for k in 1..=wanted {
        for (_, entity, subtree) in &safe {
            // The step is a WORLD offset applied in the entity's own parent
            // frame — a copy of a parented piece has to land where the
            // arithmetic says, not where the same numbers land at the root.
            // Only the ROOT is stepped: descendants are local to it and ride
            // along, so stepping them too would shear the copy apart.
            let parent_inverse = world
                .get::<ChildOf>(*entity)
                .and_then(|c| world.get::<GlobalTransform>(c.parent()))
                .map(|g| g.affine().inverse())
                .unwrap_or(bevy::math::Affine3A::IDENTITY);
            let local_step = parent_inverse.transform_vector3(step) * k as f32;
            let mut stamp = subtree.cloned();
            stamp.map_root_transform(|transform| Transform {
                translation: transform.translation + local_step,
                ..transform
            });
            let (mut made, root) = copy_ops(&stamp, stamp.external_parent);
            ops.append(&mut made);
            new_roots.push(root);
        }
    }
    if ops.is_empty() {
        say(world, "array needs a count".into(), false);
        return;
    }
    let laid = new_roots.len();
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
        .0 = new_roots;
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
    if derived > 0 {
        message.push_str(&format!(" \u{b7} {derived} skipped (generated parts)"));
    }
    if lossy > 0 {
        message.push_str(&format!(
            " \u{b7} {lossy} skipped (parts inside a generated subtree)"
        ));
    }
    // Hidden-ness is a view, not a component, so fresh ids come back VISIBLE.
    // Say so rather than let a run quietly reveal what someone hid.
    let hidden_inside = {
        let hidden = world.resource::<editor_core::hide::Hidden>();
        safe.iter()
            .flat_map(|(_, _, subtree)| subtree.records.iter())
            .filter(|record| hidden.contains(record.id))
            .count()
    };
    if hidden_inside > 0 {
        message.push_str(&format!(
            " \u{b7} {hidden_inside} hidden part{} came back visible",
            if hidden_inside == 1 { "" } else { "s" }
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

    /// A group arrays as a GROUP.
    ///
    /// This replaces the test that pinned the opposite: array used to copy one
    /// entity, so it folded nothing and refused any root with real children.
    /// Selecting a parent AND its child now folds to the parent, and each copy
    /// is the whole subtree — two entities per copy, not one, and not four
    /// loose pieces.
    #[test]
    fn a_group_arrays_as_a_group() {
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
                            Box::new(Transform::from_xyz(0.5, 0.0, 0.0)).into_partial_reflect(),
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
        assert_eq!(
            after - before,
            4,
            "two copies of a two-entity group, and the child copied ONCE"
        );

        // Every copied child hangs under its OWN copy root, not the original.
        let original = app.world().resource::<SceneIndex>().get(&parent).unwrap();
        let roots: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, (With<SceneId>, Without<ChildOf>)>()
            .iter(app.world())
            .filter(|e| *e != original)
            .collect();
        assert_eq!(roots.len(), 2, "the copies did not land as roots");
        for root in roots {
            let kids = app
                .world()
                .get::<Children>(root)
                .map(|c| c.iter().count())
                .unwrap_or(0);
            assert_eq!(kids, 1, "a copy came out without its child");
        }
    }

    /// Only the ROOT steps. Descendants are local to it and ride along, so a
    /// walker that stepped them too would shear every copy apart — and with no
    /// `TransformPlugin` in the test app, the LOCAL assertion is the one that
    /// can see it.
    #[test]
    fn arraying_a_group_steps_only_the_root() {
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
                        components: vec![
                            Box::new(Transform::from_xyz(1.0, 0.0, 0.0)).into_partial_reflect(),
                        ],
                    },
                    Op::Spawn {
                        id: child,
                        components: vec![
                            Box::new(Transform::from_xyz(0.5, 0.0, 0.0)).into_partial_reflect(),
                        ],
                    },
                    Op::Reparent {
                        target: child,
                        parent: Some(parent),
                    },
                ],
            });
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&parent).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);
        perform_array(app.world_mut(), 0, 2);
        app.update();

        let grid = app.world().resource::<EditorSettings>().viewport.grid_step;
        let mut root_xs: Vec<f32> = app
            .world_mut()
            .query_filtered::<&Transform, (With<SceneId>, Without<ChildOf>)>()
            .iter(app.world())
            .map(|t| t.translation.x)
            .collect();
        root_xs.sort_by(f32::total_cmp);
        assert_eq!(root_xs.len(), 3);
        for (k, x) in root_xs.iter().enumerate() {
            let want = 1.0 + grid * k as f32;
            assert!((x - want).abs() < 1e-4, "root {k} at {x}, wanted {want}");
        }
        let child_xs: Vec<f32> = app
            .world_mut()
            .query_filtered::<&Transform, (With<SceneId>, With<ChildOf>)>()
            .iter(app.world())
            .map(|t| t.translation.x)
            .collect();
        assert_eq!(child_xs.len(), 3);
        assert!(
            child_xs.iter().all(|x| (x - 0.5).abs() < 1e-4),
            "a child was stepped as well as riding its root: {child_xs:?}"
        );
    }

    /// The narrow gate that is all that survives of the old blanket refusal:
    /// real content under a GENERATED member cannot be reached by the walk (a
    /// stamp re-mints those ids every run), so a copy would silently drop it.
    /// Deleting the gate without keeping this test is exactly how the hole
    /// comes back.
    #[test]
    fn arraying_refuses_a_root_with_real_content_under_a_stamped_member() {
        let mut app = crate::tests::test_app();
        let root = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root,
                    components: vec![Box::new(Transform::default()).into_partial_reflect()],
                }],
            });
        app.update();
        let root_entity = app.world().resource::<SceneIndex>().get(&root).unwrap();
        let member = app
            .world_mut()
            .spawn((
                SceneId::random(),
                crate::PrefabStamped,
                ChildOf(root_entity),
            ))
            .id();
        app.world_mut().spawn((SceneId::random(), ChildOf(member)));
        app.world_mut().entity_mut(root_entity).insert(Selected);

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
        assert_eq!(after, before, "array copied a root it would have gutted");
    }

    /// A prefab instance copies as ONE entity: its members are generated, and
    /// the copy's own stamp rebuilds them.
    #[test]
    fn arraying_a_prefab_instance_lays_one_entity_per_copy() {
        let mut app = crate::tests::test_app();
        let root = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root,
                    components: vec![Box::new(Transform::default()).into_partial_reflect()],
                }],
            });
        app.update();
        let root_entity = app.world().resource::<SceneIndex>().get(&root).unwrap();
        app.world_mut().spawn((
            SceneId::random(),
            crate::PrefabStamped,
            ChildOf(root_entity),
        ));
        app.world_mut().entity_mut(root_entity).insert(Selected);

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
        assert_eq!(
            after - before,
            2,
            "the generated member was copied instead of being left to regenerate"
        );
    }
}
