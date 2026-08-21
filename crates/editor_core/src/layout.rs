//! What an assisted-layout verb is allowed to act on (spec §9).
//!
//! Array, mirror and everything after them ask the same three questions: what
//! does this selection actually NAME, what may I touch, and what do I owe the
//! user when the answer is "nothing"? One implementation, because two copies of
//! that rule is how `space h` and `*` start disagreeing about what a selection
//! means.

use crate::hide::{Hidden, is_hidden_world};
use crate::lock::Locked;
use crate::selection::{Selected, outermost_seal};
use bevy::prelude::*;
use editor_api::prelude::*;

pub const AXIS_NAMES: [&str; 3] = ["X", "Y", "Z"];

pub fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// The selection, folded to what it actually names — the same rule hide uses,
/// so a socket handle acts on its piece and a stamped member acts on its
/// instance. Deduped by `SceneId`, order preserved.
pub fn folded_selection(world: &mut World) -> Vec<(SceneId, Entity)> {
    let selected: Vec<Entity> = world
        .query_filtered::<Entity, (With<Selected>, With<SceneId>)>()
        .iter(world)
        .collect();
    let mut out: Vec<(SceneId, Entity)> = Vec::new();
    for entity in selected {
        let resolved = outermost_seal(world, entity);
        let Some(id) = world.get::<SceneId>(resolved).copied() else {
            continue;
        };
        if !out.iter().any(|(seen, _)| *seen == id) {
            out.push((id, resolved));
        }
    }
    out
}

/// A folded selection split into what may be touched and what may not, with the
/// refusals COUNTED — a verb that reports "mirrored 3" after skipping two reads
/// as complete success.
pub struct Subjects {
    pub subject: Vec<(SceneId, Entity)>,
    pub locked: usize,
    pub hidden: usize,
}

/// Locked objects are skipped rather than refused-at-the-queue, because
/// `Op::Spawn` is not an edit TO anything and the lock would never stop an
/// array; and hidden objects are skipped because a layout verb must not be the
/// one thing that materialises what you took out of the view.
pub fn subjects(world: &mut World) -> Subjects {
    let roots = folded_selection(world);
    let hidden_set = world.resource::<Hidden>().clone();
    let mut out = Subjects {
        subject: Vec::new(),
        locked: 0,
        hidden: 0,
    };
    for (id, entity) in roots {
        if world.get::<Locked>(entity).is_some() {
            out.locked += 1;
        } else if is_hidden_world(world, entity, &hidden_set) {
            out.hidden += 1;
        } else {
            out.subject.push((id, entity));
        }
    }
    out
}

/// `subjects`, minus the ones another subject already carries.
///
/// For verbs that write a POSE onto an EXISTING transform — mirror, drop. Verbs
/// that SPAWN keep using `subjects`: a parent and a child are two independent
/// things to copy, and copies do not compound the way a delta does.
///
/// AFTER the locked/hidden split, never before. `subjects` has already dropped
/// locked entities, so a locked parent is not in the carrier set and its
/// unlocked selected child correctly stays a subject — otherwise "moving ten
/// objects with two locked moves the eight" would silently become "moves none".
pub fn transform_subjects(world: &mut World) -> Subjects {
    let mut found = subjects(world);
    let carriers: Vec<Entity> = found.subject.iter().map(|(_, entity)| *entity).collect();
    let read: &World = world;
    found.subject.retain(|(_, entity)| {
        !crate::selection::carried_by(
            *entity,
            |ancestor| carriers.contains(&ancestor),
            |e| read.get::<ChildOf>(e).map(|c| c.parent()),
        )
    });
    found
}

/// What a verb owes the user when it has nothing left to act on. Every refusal
/// names the way OUT, not just the problem.
pub fn refusal(subjects: &Subjects, verb: &str) -> Option<String> {
    if !subjects.subject.is_empty() {
        return None;
    }
    Some(if subjects.locked > 0 && subjects.hidden == 0 {
        format!(
            "{} locked object{} \u{b7} \u{2423}l to unlock",
            subjects.locked,
            plural(subjects.locked)
        )
    } else if subjects.hidden > 0 && subjects.locked == 0 {
        "the selection is hidden \u{b7} \u{2423}u to unhide".to_string()
    } else if subjects.locked > 0 {
        "the selection is locked or hidden".to_string()
    } else {
        format!("select something to {verb}")
    })
}

/// The tail a partially-skipped run owes its message.
pub fn skipped_note(subjects: &Subjects) -> String {
    let mut note = String::new();
    if subjects.locked > 0 {
        note.push_str(&format!(" \u{b7} {} skipped (locked)", subjects.locked));
    }
    if subjects.hidden > 0 {
        note.push_str(&format!(" \u{b7} {} skipped (hidden)", subjects.hidden));
    }
    note
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(subject: usize, locked: usize, hidden: usize) -> Subjects {
        Subjects {
            subject: (0..subject)
                .map(|_| (SceneId::random(), Entity::PLACEHOLDER))
                .collect(),
            locked,
            hidden,
        }
    }

    /// Refusals name the way OUT, not just the problem.
    #[test]
    fn refusals_say_what_to_do_about_it() {
        assert!(
            refusal(&found(1, 2, 0), "mirror").is_none(),
            "it had work to do"
        );
        assert!(
            refusal(&found(0, 1, 0), "mirror")
                .unwrap()
                .contains("\u{2423}l")
        );
        assert!(
            refusal(&found(0, 0, 1), "mirror")
                .unwrap()
                .contains("\u{2423}u")
        );
        assert_eq!(
            refusal(&found(0, 0, 0), "mirror").unwrap(),
            "select something to mirror"
        );
    }

    /// A partial skip is REPORTED. Silently mirroring three of five and saying
    /// "mirrored 3" reads as success.
    #[test]
    fn a_partial_run_says_what_it_left_out() {
        let note = skipped_note(&found(3, 2, 1));
        assert!(note.contains("2 skipped (locked)") && note.contains("1 skipped (hidden)"));
        assert!(skipped_note(&found(3, 0, 0)).is_empty());
    }
}
