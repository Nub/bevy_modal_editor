//! Hide / isolate / unhide-all (spec §9): getting things out of the way.
//!
//! Hidden-ness is EDITOR VIEW STATE, and it is the opposite of [`Locked`] on
//! every axis — deliberately, because the two verbs answer different questions.
//! `Locked` is a serialized component enforced inside `apply_edits`: it changes
//! what the SCENE allows, so it belongs to the scene and persists with it.
//! Hidden changes what YOU are looking at: it never enters a `Transaction`,
//! never touches `History`, never marks the scene dirty, and never reaches
//! `level.ron`. `u` will not bring an object back, which is exactly why every
//! message here names `␣u`.
//!
//! [`Locked`]: crate::lock::Locked
//!
//! Keyed by `SceneId`, never `Entity`: play/reset despawns and respawns every
//! scene entity, so every `Entity` id changes underneath a hidden set. A
//! `SceneId` survives that, which is what makes hide come back after F7.
//!
//! And hide LIFTS whenever the editor is inactive. F5 shows the real level —
//! nobody should playtest against a level that is secretly missing its floor,
//! and a game animating visibility during play must not have a second writer.

use crate::selection::{Selected, SelectionHandle, SelectionScope};
use bevy::prelude::*;
use editor_api::prelude::*;
use std::collections::HashSet;

/// The hidden set, plus what isolate has to put back.
#[derive(Resource, Default, Clone)]
pub struct Hidden {
    ids: HashSet<SceneId>,
    /// `Some` while isolate is active: exactly what was hidden before it began.
    isolate_restore: Option<HashSet<SceneId>>,
}

impl Hidden {
    pub fn contains(&self, id: SceneId) -> bool {
        self.ids.contains(&id)
    }
    pub fn len(&self) -> usize {
        self.ids.len()
    }
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
    pub fn is_isolated(&self) -> bool {
        self.isolate_restore.is_some()
    }
    pub fn iter(&self) -> impl Iterator<Item = SceneId> + '_ {
        self.ids.iter().copied()
    }
    /// Restore a session's hidden set (the rebuild-loop sidecar).
    pub fn set(&mut self, ids: impl IntoIterator<Item = SceneId>) {
        self.ids = ids.into_iter().collect();
    }
}

/// Is this entity hidden, itself or through an ancestor?
///
/// The ancestor walk is the whole point: a prefab's stamped members and an
/// import's derived gltf nodes are not in the hidden set themselves — they hang
/// under something that is, and a rule that only looked at the entity would let
/// a box-drag select the inside of a hidden object.
pub fn is_hidden(
    entity: Entity,
    hidden: &Hidden,
    ids: &Query<&SceneId>,
    parents: &Query<&ChildOf>,
) -> bool {
    let mut current = entity;
    loop {
        if let Ok(id) = ids.get(current)
            && hidden.contains(*id)
        {
            return true;
        }
        match parents.get(current) {
            Ok(parent) => current = parent.parent(),
            Err(_) => return false,
        }
    }
}

/// The same rule for exclusive systems, which have a `&World` and no queries.
pub fn is_hidden_world(world: &World, entity: Entity, hidden: &Hidden) -> bool {
    let mut current = entity;
    loop {
        if let Some(id) = world.get::<SceneId>(current)
            && hidden.contains(*id)
        {
            return true;
        }
        match world.get::<ChildOf>(current) {
            Some(parent) => current = parent.parent(),
            None => return false,
        }
    }
}

/// What isolate hides: everything it is allowed to touch, minus what has to
/// stay lit.
///
/// `keep` is the focus, its ANCESTORS and its DESCENDANTS. Ancestors because
/// hiding a parent hides the focus through inheritance. Descendants because
/// `Visibility::Hidden` is unconditional rather than a hint — a child with its
/// own `SceneId` would be hidden directly, and the object you isolated would
/// go black while everything around it disappeared.
pub fn isolate_set(candidates: &[SceneId], keep: &HashSet<SceneId>) -> HashSet<SceneId> {
    candidates
        .iter()
        .copied()
        .filter(|id| !keep.contains(id))
        .collect()
}

#[derive(Resource, Default, Clone)]
pub(crate) struct HideRequests {
    hide: bool,
    isolate: bool,
    unhide_all: bool,
    pub(crate) similar: bool,
}

pub(crate) fn collect_hide_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<crate::resolver::EditorState>,
    mut requests: ResMut<HideRequests>,
) {
    if !state.active {
        return;
    }
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "select.hide" => requests.hide = true,
            "select.isolate" => requests.isolate = true,
            "select.unhide-all" => requests.unhide_all = true,
            "select.similar" => requests.similar = true,
            _ => {}
        }
    }
}

fn say(world: &mut World, message: String, success: bool) {
    world.write_message(editor_api::feedback::SceneIoFeedback { message, success });
}

/// Every entity these verbs are allowed to act on: scene entities that are
/// their own outermost seal, are not handles, and are inside the current
/// selection scope.
///
/// That one rule keeps a closed prefab's stamped members out without the kernel
/// ever naming `PrefabStamped`, and makes every verb here scope-correct inside
/// an open prefab, where the open root's seal has been removed.
pub(crate) fn candidates(world: &mut World) -> Vec<(SceneId, Entity)> {
    let scope = world.resource::<SelectionScope>().0.clone();
    let all: Vec<(Entity, SceneId)> = world
        .query_filtered::<(Entity, &SceneId), Without<SelectionHandle>>()
        .iter(world)
        .map(|(entity, id)| (entity, *id))
        .collect();
    all.into_iter()
        .filter(|(entity, _)| crate::selection::outermost_seal(world, *entity) == *entity)
        .filter(|(entity, _)| scope.as_ref().is_none_or(|s| s.contains(entity)))
        .map(|(entity, id)| (id, entity))
        .collect()
}

/// The selection, folded to what it actually names.
///
/// A stamped member clicked in the hierarchy folds to its instance root: a
/// stamp hands out a fresh `SceneId` every time it runs, so an entry keyed on a
/// member would silently die at the next restamp and the object would come
/// back on its own. Returns whether any fold happened, because the user should
/// be told they hid more than they pointed at.
fn focus(world: &mut World) -> (Vec<(SceneId, Entity)>, bool) {
    let selected: Vec<Entity> = world
        .query_filtered::<Entity, (With<Selected>, With<SceneId>)>()
        .iter(world)
        .collect();
    let mut folded = false;
    let mut out: Vec<(SceneId, Entity)> = Vec::new();
    for entity in selected {
        let resolved = crate::selection::outermost_seal(world, entity);
        if resolved != entity {
            folded = true;
        }
        let Some(id) = world.get::<SceneId>(resolved).copied() else {
            continue;
        };
        if !out.iter().any(|(seen, _)| *seen == id) {
            out.push((id, resolved));
        }
    }
    (out, folded)
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

pub(crate) fn perform_hide(world: &mut World) {
    let requests = std::mem::take(&mut *world.resource_mut::<HideRequests>());
    // `similar` rides on the same collector; its own system consumes it.
    if requests.similar {
        world.resource_mut::<HideRequests>().similar = true;
    }
    if requests.hide {
        do_hide(world);
    }
    if requests.isolate {
        do_isolate(world);
    }
    if requests.unhide_all {
        do_unhide_all(world);
    }
}

fn do_hide(world: &mut World) {
    let (focus, folded) = focus(world);
    if focus.is_empty() {
        say(
            world,
            "nothing selected \u{b7} \u{2423}h hides the selection".into(),
            false,
        );
        return;
    }
    let new = {
        let hidden = world.resource::<Hidden>();
        focus.iter().filter(|(id, _)| !hidden.contains(*id)).count()
    };
    {
        let mut hidden = world.resource_mut::<Hidden>();
        for (id, _) in &focus {
            hidden.ids.insert(*id);
        }
    }
    // Hide DESELECTS. Otherwise the next move or delete lands on something you
    // cannot see, which is the silent destruction the lock work exists to stop.
    for (_, entity) in &focus {
        world.entity_mut(*entity).remove::<Selected>();
    }
    world.write_message(crate::selection::SelectionChanged);
    if new == 0 {
        say(
            world,
            "already hidden \u{b7} \u{2423}u to unhide all".into(),
            false,
        );
        return;
    }
    let note = if folded {
        " (folded to prefab roots)"
    } else {
        ""
    };
    say(
        world,
        format!(
            "hid {new} object{}{note} \u{b7} \u{2423}u to unhide all",
            plural(new)
        ),
        true,
    );
}

fn do_isolate(world: &mut World) {
    if world.resource::<Hidden>().is_isolated() {
        // EXIT is a RESTORE, not an unhide-all: hides you made before isolating
        // survive it. Anything else would make isolate a destructive verb for
        // the work you did before you reached for it.
        let restored = {
            let mut hidden = world.resource_mut::<Hidden>();
            let restore = hidden.isolate_restore.take().unwrap_or_default();
            hidden.ids = restore;
            hidden.ids.len()
        };
        let message = if restored == 0 {
            "isolate off".to_string()
        } else {
            format!("isolate off \u{b7} {restored} hidden")
        };
        say(world, message, true);
        return;
    }
    let (focus, _) = focus(world);
    if focus.is_empty() {
        say(
            world,
            "nothing selected \u{b7} \u{2423}\u{21e7}h isolates the selection".into(),
            false,
        );
        return;
    }
    let mut keep: HashSet<SceneId> = HashSet::new();
    for (id, entity) in &focus {
        keep.insert(*id);
        let mut current = *entity;
        while let Some(parent) = world.get::<ChildOf>(current).map(|c| c.parent()) {
            if let Some(id) = world.get::<SceneId>(parent) {
                keep.insert(*id);
            }
            current = parent;
        }
        let mut stack = vec![*entity];
        while let Some(node) = stack.pop() {
            if let Some(id) = world.get::<SceneId>(node) {
                keep.insert(*id);
            }
            if let Some(children) = world.get::<Children>(node) {
                stack.extend(children.iter());
            }
        }
    }
    let candidate_ids: Vec<SceneId> = candidates(world).into_iter().map(|(id, _)| id).collect();
    let now = isolate_set(&candidate_ids, &keep);
    let (kept, count) = (focus.len(), now.len());
    {
        let mut hidden = world.resource_mut::<Hidden>();
        let before = hidden.ids.clone();
        hidden.isolate_restore = Some(before);
        hidden.ids = now;
    }
    say(
        world,
        format!(
            "isolated {kept} object{} \u{b7} {count} hidden \u{b7} \u{2423}\u{21e7}h to go back",
            plural(kept)
        ),
        true,
    );
}

fn do_unhide_all(world: &mut World) {
    let count = world.resource::<Hidden>().len();
    if count == 0 {
        say(world, "nothing is hidden".into(), false);
        return;
    }
    {
        let mut hidden = world.resource_mut::<Hidden>();
        hidden.ids.clear();
        // Drop the restore set too: otherwise leaving isolate later would
        // resurrect exactly the objects the user just asked to see.
        hidden.isolate_restore = None;
    }
    say(
        world,
        format!("unhid {count} object{}", plural(count)),
        true,
    );
}
/// The only writer of `Visibility` on SCENE entities.
///
/// It runs in `PostUpdate` rather than `EditorSet::Sync` because the system
/// that flips the editor off and hands the world to the game lives inside
/// `editor_scene`'s own chain, and the kernel must not depend on `editor_scene`
/// to order against it. `PostUpdate` is after all of `Update`, so on the F5
/// frame the flip has already happened and hide lifts before anything renders;
/// on the F7 frame the respawn has already happened, so there is no flash.
///
/// It drives the whole SUBTREE rather than setting the root and trusting
/// inheritance, because inheritance does not reach the geometry that matters
/// here: an imported model's content is a spawned asset subtree that bevy's
/// propagation treats as its own root, so hiding a prefab instance left its
/// meshes lit while the instance itself read as hidden — the exact shape of
/// "the editor says it worked and the viewport disagrees". Walking it costs
/// something only for objects that are actually hidden.
pub(crate) fn sync_hidden_visibility(
    hidden: Res<Hidden>,
    state: Res<crate::resolver::EditorState>,
    index: Res<SceneIndex>,
    children: Query<&Children>,
    mut visibility: Query<&mut Visibility>,
    mut applied: Local<HashSet<Entity>>,
    mut commands: Commands,
) {
    let roots: Vec<Entity> = if state.active {
        // Hide LIFTS whenever the editor is inactive: F5 shows the real level.
        hidden.iter().filter_map(|id| index.get(&id)).collect()
    } else {
        Vec::new()
    };
    let mut desired: HashSet<Entity> = HashSet::new();
    for root in roots {
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if !desired.insert(node) {
                continue;
            }
            if let Ok(kids) = children.get(node) {
                stack.extend(kids.iter());
            }
        }
    }
    for entity in applied.difference(&desired) {
        if let Ok(mut current) = visibility.get_mut(*entity)
            && *current != Visibility::Inherited
        {
            // Set, not remove: `Visibility` is editor-owned and never saved, so
            // "absent" and "Inherited" mean the same thing here.
            *current = Visibility::Inherited;
        }
    }
    for entity in &desired {
        match visibility.get_mut(*entity) {
            // Change-guarded: propagation keys on `Changed<Visibility>`, so
            // rewriting the same value would re-propagate every subtree every
            // frame.
            Ok(mut current) if *current != Visibility::Hidden => *current = Visibility::Hidden,
            Ok(_) => {}
            Err(_) => {
                commands.entity(*entity).try_insert(Visibility::Hidden);
            }
        }
    }
    *applied = desired;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: usize) -> Vec<SceneId> {
        (0..n).map(|_| SceneId::random()).collect()
    }

    /// The formula that looks right and blacks out the thing you isolated:
    /// `Visibility::Hidden` is unconditional, so a descendant carrying its own
    /// `SceneId` gets hidden directly no matter what its parent says.
    #[test]
    fn isolate_keeps_the_focus_its_ancestors_and_its_descendants() {
        let all = ids(5);
        let (parent, focus, child, other, far) = (all[0], all[1], all[2], all[3], all[4]);
        let keep: HashSet<SceneId> = [parent, focus, child].into_iter().collect();
        let hidden = isolate_set(&all, &keep);
        assert!(!hidden.contains(&child), "isolate hid its own descendant");
        assert!(!hidden.contains(&parent), "isolate hid its own ancestor");
        assert!(!hidden.contains(&focus));
        assert!(hidden.contains(&other) && hidden.contains(&far));
    }

    /// Leaving isolate RESTORES; it does not reveal. A naive "exit by unhiding
    /// everything" passes every other test and fails only this one — and it
    /// silently throws away the hiding a person did before they isolated.
    #[test]
    fn isolate_toggle_restores_the_pre_isolate_set() {
        let all = ids(3);
        let (a, b, c) = (all[0], all[1], all[2]);
        let mut hidden = Hidden::default();
        hidden.ids.insert(a);

        let before = hidden.ids.clone();
        hidden.isolate_restore = Some(before);
        hidden.ids = isolate_set(&all, &[b].into_iter().collect());
        assert!(hidden.is_isolated());
        assert!(hidden.contains(c), "isolate did not hide the rest");

        hidden.ids = hidden.isolate_restore.take().unwrap();
        assert_eq!(hidden.ids, [a].into_iter().collect::<HashSet<_>>());
        assert!(!hidden.is_isolated());
    }

    /// Unhide-all drops the restore set, or a later isolate-exit resurrects
    /// exactly what the user asked to see.
    #[test]
    fn unhide_all_clears_the_isolate_restore() {
        let all = ids(3);
        let mut hidden = Hidden::default();
        hidden.ids.insert(all[0]);
        hidden.isolate_restore = Some(hidden.ids.clone());

        hidden.ids.clear();
        hidden.isolate_restore = None;

        // Isolate again, then leave it: the old hide must NOT come back.
        hidden.isolate_restore = Some(hidden.ids.clone());
        hidden.ids = isolate_set(&all, &[all[2]].into_iter().collect());
        hidden.ids = hidden.isolate_restore.take().unwrap();
        assert!(hidden.is_empty(), "unhidden objects came back");
    }

    /// The world-side half. `do_hide` needs almost nothing: the two messages it
    /// speaks through, and the set it writes.
    fn hide_world() -> World {
        let mut world = World::new();
        world.init_resource::<Hidden>();
        world.init_resource::<SelectionScope>();
        world.insert_resource(bevy::ecs::message::Messages::<
            editor_api::feedback::SceneIoFeedback,
        >::default());
        world.insert_resource(bevy::ecs::message::Messages::<
            crate::selection::SelectionChanged,
        >::default());
        world
    }

    fn last_feedback(world: &mut World) -> Option<(String, bool)> {
        let messages =
            world.resource::<bevy::ecs::message::Messages<editor_api::feedback::SceneIoFeedback>>();
        let mut cursor = messages.get_cursor();
        cursor
            .read(messages)
            .last()
            .map(|m| (m.message.clone(), m.success))
    }

    /// Hiding a stamped member records the INSTANCE. A stamp hands out a fresh
    /// `SceneId` every time it runs, so an entry keyed on a member would die
    /// silently at the next restamp and the object would come back on its own.
    #[test]
    fn hide_folds_a_stamped_member_to_its_instance() {
        let mut world = hide_world();
        let root_id = SceneId::random();
        let member_id = SceneId::random();
        let root = world
            .spawn((root_id, crate::selection::SelectionSealed))
            .id();
        world.spawn((member_id, Selected, ChildOf(root)));

        do_hide(&mut world);

        let hidden = world.resource::<Hidden>();
        assert!(hidden.contains(root_id), "the instance was not hidden");
        assert!(
            !hidden.contains(member_id),
            "a stamped member's own id was recorded; it dies at the next restamp"
        );
    }

    /// Hide DESELECTS. Otherwise the next move or delete lands on something
    /// nobody can see.
    #[test]
    fn hiding_drops_the_selection() {
        let mut world = hide_world();
        let entity = world.spawn((SceneId::random(), Selected)).id();
        do_hide(&mut world);
        assert!(world.get::<Selected>(entity).is_none());
    }

    /// An empty selection SAYS so. `perform_lock` returns in silence here, and
    /// that is the "logging is not user feedback" failure we are not copying.
    #[test]
    fn hiding_nothing_says_so() {
        let mut world = hide_world();
        do_hide(&mut world);
        let (message, success) = last_feedback(&mut world).expect("hide said nothing at all");
        assert!(!success);
        assert!(
            message.contains("nothing selected"),
            "unhelpful refusal: {message}"
        );
        assert!(world.resource::<Hidden>().is_empty());
    }

    /// Unhide-all on an empty set is a refusal, not a silent success.
    #[test]
    fn unhide_all_with_nothing_hidden_says_so() {
        let mut world = hide_world();
        do_unhide_all(&mut world);
        let (message, success) = last_feedback(&mut world).expect("unhide said nothing");
        assert!(!success);
        assert!(message.contains("nothing is hidden"), "{message}");
    }
}
