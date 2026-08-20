//! Drop to surface (spec §9, freeform snap solvers): rest the selection on what
//! is underneath it instead of leaving it intersecting.
//!
//! A free drag moves in the camera plane, so objects pass through floors, walls
//! and each other as a matter of course — the drag has no idea anything else is
//! there. This is the verb that puts a thing DOWN.
//!
//! **Boxes, not triangles.** Resting is computed from axis-aligned bounds, so a
//! crate lands on a table's bounding box rather than on the tabletop mesh. For
//! blockout — the work this editor is for — that is the same answer nearly all
//! of the time, and it is deterministic, instant, and testable without a ray
//! caster. True surface snapping (and vertex/edge/face snapping beside it) is
//! the freeform paradigm's own slice, and this is not pretending to be it.

use bevy::prelude::*;
use editor_api::prelude::*;

/// A world-space axis-aligned box.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Bounds {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds {
    pub fn from_points(a: Vec3, b: Vec3) -> Self {
        Self {
            min: a.min(b),
            max: a.max(b),
        }
    }

    /// Do these two overlap when seen from above? Only then can one rest on the
    /// other.
    pub fn overlaps_from_above(&self, other: &Self) -> bool {
        self.min.x < other.max.x
            && other.min.x < self.max.x
            && self.min.z < other.max.z
            && other.min.z < self.max.z
    }
}

/// How far to move `mover` vertically so it rests on the highest surface
/// beneath it — the ground plane if nothing else is under it.
///
/// "Beneath" is generous on purpose: a support whose top is slightly ABOVE the
/// mover's base still counts, up to `tolerance`, because the common case for
/// this verb is an object that has sunk INTO a floor and needs lifting out of
/// it. Anything higher than that is something the object is behind, not
/// standing on, and lifting onto it would teleport the object up a wall.
pub fn drop_offset(mover: &Bounds, others: &[Bounds], ground: f32, tolerance: f32) -> f32 {
    let mut rest_on = ground;
    for other in others {
        if !mover.overlaps_from_above(other) {
            continue;
        }
        if other.max.y > mover.min.y + tolerance {
            continue; // above us: a wall we are beside, not a floor we are on
        }
        rest_on = rest_on.max(other.max.y);
    }
    rest_on - mover.min.y
}

/// World bounds of an entity's whole visual subtree.
///
/// `None` when nothing under it has geometry — a spawn point or a trigger
/// volume has nothing to rest ON, and moving it by a guess would be worse than
/// leaving it where the designer put it.
pub fn world_bounds(world: &World, root: Entity) -> Option<Bounds> {
    let mut min = Vec3::MAX;
    let mut max = Vec3::MIN;
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if let (Some(aabb), Some(global)) = (
            world.get::<bevy::camera::primitives::Aabb>(entity),
            world.get::<GlobalTransform>(entity),
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
            }
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    (min.x <= max.x).then_some(Bounds { min, max })
}

/// `transform.drop`: rest every selected root on what is under it.
pub(crate) fn perform_drop(world: &mut World) {
    if !std::mem::take(&mut world.resource_mut::<DropRequested>().0) {
        return;
    }
    let selected: Vec<(Entity, SceneId)> = world
        .query_filtered::<(Entity, &SceneId), With<editor_core::selection::Selected>>()
        .iter(world)
        .map(|(entity, id)| (entity, *id))
        .collect();
    if selected.is_empty() {
        world.write_message(crate::SceneIoFeedback {
            message: "select something to drop".into(),
            success: false,
        });
        return;
    }
    // Everything else in the scene that has a shape, measured once.
    let movers: Vec<Entity> = selected.iter().map(|(entity, _)| *entity).collect();
    let others: Vec<Bounds> = {
        let roots: Vec<Entity> = world
            .query_filtered::<Entity, With<SceneId>>()
            .iter(world)
            .filter(|entity| !movers.iter().any(|mover| is_within(world, *entity, *mover)))
            .collect();
        roots
            .into_iter()
            .filter_map(|entity| world_bounds(world, entity))
            .collect()
    };
    let mut ops = Vec::new();
    let mut dropped = 0usize;
    for (entity, id) in &selected {
        let Some(bounds) = world_bounds(world, *entity) else {
            continue;
        };
        let offset = drop_offset(&bounds, &others, 0.0, DROP_TOLERANCE);
        if offset.abs() < 1e-4 {
            continue; // already resting: not an edit, and not an undo entry
        }
        let Some(transform) = world.get::<Transform>(*entity).copied() else {
            continue;
        };
        let mut moved = transform;
        moved.translation.y += offset;
        ops.push(editor_api::edits::Op::Set {
            target: *id,
            value: Box::new(moved).into_partial_reflect(),
        });
        dropped += 1;
    }
    if ops.is_empty() {
        world.write_message(crate::SceneIoFeedback {
            message: "already resting on the surface".into(),
            success: true,
        });
        return;
    }
    world
        .resource_mut::<editor_api::edits::EditQueue>()
        .0
        .push(editor_api::edits::Transaction {
            label: "Drop To Surface".into(),
            gesture: None,
            ops,
        });
    world.write_message(crate::SceneIoFeedback {
        message: format!("dropped {dropped} onto the surface"),
        success: true,
    });
}

/// How far ABOVE an object's base a surface may be and still count as the thing
/// it should stand on. Half a metre covers a prop sunk into a floor; more would
/// start lifting things onto the walls beside them.
const DROP_TOLERANCE: f32 = 0.5;

fn is_within(world: &World, entity: Entity, ancestor: Entity) -> bool {
    let mut current = entity;
    loop {
        if current == ancestor {
            return true;
        }
        match world.get::<ChildOf>(current) {
            Some(parent) => current = parent.parent(),
            None => return false,
        }
    }
}

#[derive(Resource, Default)]
pub(crate) struct DropRequested(pub bool);

pub(crate) fn collect_drop_action(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<editor_core::prelude::EditorState>,
    mut requested: ResMut<DropRequested>,
) {
    for invoked in reader.read() {
        if state.active && invoked.action.as_str() == "transform.drop" {
            requested.0 = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_at(centre: Vec3, half: Vec3) -> Bounds {
        Bounds::from_points(centre - half, centre + half)
    }

    /// The everyday case: a crate floating above a floor comes down onto it.
    #[test]
    fn a_floating_object_lands_on_what_is_under_it() {
        let floor = box_at(Vec3::new(0.0, -0.1, 0.0), Vec3::new(10.0, 0.1, 10.0));
        let crate_box = box_at(Vec3::new(0.0, 4.0, 0.0), Vec3::splat(0.5));
        let offset = drop_offset(&crate_box, &[floor], 0.0, 0.5);
        assert!((offset - (-3.5)).abs() < 1e-5, "{offset}");
    }

    /// And the case that prompted it: something SUNK into a surface comes back
    /// out of it, rather than being left clipping.
    #[test]
    fn a_sunk_object_is_lifted_out() {
        let table = box_at(Vec3::new(0.0, 0.5, 0.0), Vec3::new(2.0, 0.5, 2.0));
        let mug = box_at(Vec3::new(0.0, 0.9, 0.0), Vec3::splat(0.1));
        let offset = drop_offset(&mug, &[table], 0.0, 0.5);
        assert!(offset > 0.0, "it rose: {offset}");
        assert!((mug.min.y + offset - 1.0).abs() < 1e-5, "onto the top");
    }

    /// A wall beside you is not a surface to stand on. Lifting onto it would
    /// teleport the object up the wall, which is the failure this tolerance
    /// exists to avoid.
    #[test]
    fn a_neighbour_that_towers_over_you_is_not_a_floor() {
        let wall = box_at(Vec3::new(0.0, 5.0, 0.0), Vec3::new(1.0, 5.0, 1.0));
        let crate_box = box_at(Vec3::new(0.5, 1.0, 0.5), Vec3::splat(0.5));
        let offset = drop_offset(&crate_box, &[wall], 0.0, 0.5);
        assert!(
            (offset - (-0.5)).abs() < 1e-5,
            "fell to the ground: {offset}"
        );
    }

    /// Only what is actually beneath you counts.
    #[test]
    fn something_off_to_the_side_holds_nothing_up() {
        let plinth = box_at(Vec3::new(20.0, 1.0, 0.0), Vec3::splat(1.0));
        let crate_box = box_at(Vec3::new(0.0, 3.0, 0.0), Vec3::splat(0.5));
        let offset = drop_offset(&crate_box, &[plinth], 0.0, 0.5);
        assert!((offset - (-2.5)).abs() < 1e-5, "onto the ground: {offset}");
    }

    /// The HIGHEST thing under you wins — a crate on a table on a floor rests
    /// on the table.
    #[test]
    fn the_highest_support_wins() {
        let floor = box_at(Vec3::new(0.0, -0.1, 0.0), Vec3::new(10.0, 0.1, 10.0));
        let table = box_at(Vec3::new(0.0, 0.5, 0.0), Vec3::new(2.0, 0.5, 2.0));
        let crate_box = box_at(Vec3::new(0.0, 4.0, 0.0), Vec3::splat(0.5));
        let offset = drop_offset(&crate_box, &[floor, table], 0.0, 0.5);
        assert!((crate_box.min.y + offset - 1.0).abs() < 1e-5);
    }
}
