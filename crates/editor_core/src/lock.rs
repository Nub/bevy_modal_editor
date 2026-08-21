//! Locking (owner ask): "I want to be able to lock objects to prevent further
//! editing", and to lock a whole selection at once.
//!
//! A lock is enforced at the ONE place every mutation passes through —
//! `apply_edits`. That is the entire implementation, and it is why the lock
//! cannot be got around: there is no side door to the scene (spec §8), so a
//! guard on the queue covers move, rotate, scale, delete, reparent, patch,
//! socket mating, drop, painting and every verb written after this one, with no
//! per-verb checks to forget.
//!
//! It is a SERIALIZED component, not editor state: a floor you locked stays
//! locked when the level is reopened, which is the point of locking a floor.
//!
//! Deliberate limits: a locked object is still SELECTABLE — you have to be able
//! to select it to unlock it, and to read its values — and a click on one is
//! ignored rather than passing through to whatever is behind it (that needs the
//! full hit list, not the resolved target). What it cannot be is CHANGED.

use bevy::prelude::*;
use editor_api::prelude::*;

/// This object refuses edits.
#[derive(Component, Reflect, Clone, Copy, Default, PartialEq, Debug)]
#[reflect(Component, Default)]
pub struct Locked;

/// Does this op change `target`, and is the target locked?
///
/// Unlocking is the exception that has to exist: removing the lock is an edit
/// to a locked object, and refusing it would make the lock permanent.
/// `holds_a_lock(id)` means "id, or anything in its subtree, is locked".
///
/// Only DESPAWN needs it, and that is the whole rule: despawn is the one op
/// that destroys things it does not name. Every other op edits exactly the
/// target it names, so a locked CHILD is untouched when its parent moves — it
/// rides, and riding is not an edit (spec §9, the carried-operand fold).
pub fn op_is_refused(
    op: &editor_api::edits::Op,
    is_locked: impl Fn(SceneId) -> bool,
    holds_a_lock: impl Fn(SceneId) -> bool,
) -> bool {
    use editor_api::edits::Op;
    match op {
        // Spawning is not an edit TO anything.
        Op::Spawn { .. } => false,
        Op::Remove { target, type_path } => {
            type_path != <Locked as bevy::reflect::TypePath>::type_path() && is_locked(*target)
        }
        Op::Set { target, .. } | Op::Patch { target, .. } => is_locked(*target),
        // A recursive despawn takes the whole subtree, so deleting an unlocked
        // group would quietly destroy the locked piece inside it — and the
        // lock promises to refuse EVERY edit until it is lifted.
        Op::Despawn { id } => holds_a_lock(*id),
        Op::Reparent { target, .. } => is_locked(*target),
    }
}

#[derive(Resource, Default)]
pub(crate) struct LockRequests {
    pub toggle: bool,
}

pub(crate) fn collect_lock_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<crate::resolver::EditorState>,
    mut requests: ResMut<LockRequests>,
) {
    if !state.active {
        return;
    }
    for invoked in reader.read() {
        if invoked.action.as_str() == "object.lock" {
            requests.toggle = true;
        }
    }
}

/// `l` — lock or unlock the whole selection, in ONE undoable transaction.
///
/// Mixed selections LOCK: with some locked and some not, the intent of pressing
/// lock is to end up with everything locked, not to invert each object.
pub(crate) fn perform_lock(world: &mut World) {
    if !std::mem::take(&mut world.resource_mut::<LockRequests>().toggle) {
        return;
    }
    let selected: Vec<(SceneId, bool)> = world
        .query_filtered::<(&SceneId, Option<&Locked>), With<crate::selection::Selected>>()
        .iter(world)
        .map(|(id, locked)| (*id, locked.is_some()))
        .collect();
    if selected.is_empty() {
        return;
    }
    let all_locked = selected.iter().all(|(_, locked)| *locked);
    let type_path = <Locked as bevy::reflect::TypePath>::type_path().to_string();
    let ops: Vec<editor_api::edits::Op> = selected
        .iter()
        .filter(|(_, locked)| *locked == all_locked)
        .map(|(id, _)| {
            if all_locked {
                editor_api::edits::Op::Remove {
                    target: *id,
                    type_path: type_path.clone(),
                }
            } else {
                editor_api::edits::Op::Set {
                    target: *id,
                    value: Box::new(Locked).into_partial_reflect(),
                }
            }
        })
        .collect();
    if ops.is_empty() {
        return;
    }
    let count = ops.len();
    world
        .resource_mut::<editor_api::edits::EditQueue>()
        .0
        .push(editor_api::edits::Transaction {
            label: if all_locked { "Unlock" } else { "Lock" }.into(),
            gesture: None,
            ops,
        });
    // Logging is not user feedback (spec §8): the whole point of a lock is that
    // later edits do nothing, so the moment it goes on has to be visible.
    world.write_message(editor_api::feedback::SceneIoFeedback {
        message: format!(
            "{} {count} object{} \u{00b7} \u{2423}l to {}",
            if all_locked { "unlocked" } else { "locked" },
            if count == 1 { "" } else { "s" },
            if all_locked { "lock" } else { "unlock" },
        ),
        success: true,
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_api::edits::Op;

    fn locked_set() -> Vec<SceneId> {
        vec![SceneId::random()]
    }

    #[test]
    fn a_locked_object_refuses_every_kind_of_edit() {
        let locked = locked_set();
        let target = locked[0];
        let is_locked = |id: SceneId| locked.contains(&id);
        assert!(op_is_refused(
            &Op::Set {
                target,
                value: Box::new(Transform::default()).into_partial_reflect()
            },
            is_locked,
            is_locked,
        ));
        assert!(op_is_refused(
            &Op::Patch {
                target,
                type_path: "T".into(),
                path: "x".into(),
                value: Box::new(1.0f32)
            },
            is_locked,
            is_locked,
        ));
        assert!(op_is_refused(
            &Op::Despawn { id: target },
            is_locked,
            is_locked
        ));
        assert!(op_is_refused(
            &Op::Reparent {
                target,
                parent: None
            },
            is_locked,
            is_locked,
        ));
    }

    /// Removing the lock is an edit to a locked object. Refusing it would make
    /// the lock permanent, which is a trap rather than a tool.
    #[test]
    fn unlocking_is_always_allowed() {
        let locked = locked_set();
        let is_locked = |id: SceneId| locked.contains(&id);
        assert!(!op_is_refused(
            &Op::Remove {
                target: locked[0],
                type_path: <Locked as bevy::reflect::TypePath>::type_path().into(),
            },
            is_locked,
            is_locked,
        ));
        // But removing anything ELSE from it is still refused.
        assert!(op_is_refused(
            &Op::Remove {
                target: locked[0],
                type_path: "some::other::Component".into(),
            },
            is_locked,
            is_locked,
        ));
    }

    /// Everything not locked is untouched — a lock is not a mode.
    #[test]
    fn unlocked_objects_are_unaffected() {
        let locked = locked_set();
        let other = SceneId::random();
        let is_locked = |id: SceneId| locked.contains(&id);
        assert!(!op_is_refused(
            &Op::Despawn { id: other },
            is_locked,
            is_locked
        ));
    }
}
