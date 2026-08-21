//! Selection clipboard (keymap doc: "the selection is the text object") — `d`
//! deletes/cuts the selection into the clipboard, `y` yanks it, `p` pastes copies.
//! Every mutation is an `EditScope` transaction: one undo entry per cut, one per
//! paste, restored exactly.

use bevy::prelude::*;
use editor_api::prelude::*;

use crate::edits::EditorComponents;
use crate::resolver::EditorState;
use crate::selection::{Selected, SelectionChanged};

/// Captured component sets (registered components only — the same capture set as
/// despawn inverses and serialization).
#[derive(Resource, Default)]
pub struct EditorClipboard {
    pub entries: Vec<Vec<Box<dyn PartialReflect>>>,
}

#[derive(Resource, Default)]
pub(crate) struct ClipboardRequests {
    cut: bool,
    yank: bool,
    paste: bool,
    duplicate: bool,
}

/// New ids from the last paste — selected once the spawns have applied.
#[derive(Resource, Default)]
/// The kernel's "select what was just created" channel: paste, duplicate and
/// array all hand new ids here and `select_pending` drains it once the spawns
/// resolve.
pub struct PendingPasteSelect(pub Vec<SceneId>);

/// A duplicate hands the copies straight to a move gesture (the DCC idiom:
/// duplicate, then place). It cannot start the gesture in the same frame that
/// selects them — `Selected` is inserted through commands and the gesture reads
/// the selection — so this counts down one frame first.
#[derive(Resource, Default)]
pub(crate) struct PendingDuplicateGrab(Option<u8>);

pub(crate) fn collect_clipboard_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<EditorState>,
    mut requests: ResMut<ClipboardRequests>,
) {
    if !state.active {
        return;
    }
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "select.delete" => requests.cut = true,
            "select.yank" => requests.yank = true,
            "select.paste" => requests.paste = true,
            "select.duplicate" => requests.duplicate = true,
            _ => {}
        }
    }
}

fn say(world: &mut World, message: String, success: bool) {
    world.write_message(editor_api::feedback::SceneIoFeedback { message, success });
}

/// A refusal that names what it could not take and why, in the house style.
fn copy_refusal_message(verb: &str, derived: usize, lossy: usize) -> String {
    let plural = |n: usize| if n == 1 { "" } else { "s" };
    match (derived, lossy) {
        (d, 0) if d > 0 => format!(
            "cannot {verb} {d} generated part{} \u{b7} select the object it belongs to",
            plural(d)
        ),
        (0, l) if l > 0 => format!(
            "cannot {verb} {l} object{} with parts inside a generated subtree",
            plural(l)
        ),
        (d, l) if d > 0 && l > 0 => format!(
            "cannot {verb} {} object{} \u{b7} they are generated, or have parts inside \
             something generated",
            d + l,
            plural(d + l)
        ),
        _ => format!("select something to {verb}"),
    }
}

/// Fold the selection to copyable roots and capture each one's subtree.
///
/// The one entry point for every copy verb in the kernel: returns the folded
/// subjects (so the caller can speak the locked/hidden refusal), the captures
/// that succeeded, and a count of each kind that did not.
pub(crate) fn capture_copy_set(
    world: &mut World,
) -> (
    crate::layout::Subjects,
    Vec<crate::edits::CopySubtree>,
    usize,
    usize,
) {
    let found = crate::layout::copy_subjects(world);
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let components = world.resource::<EditorComponents>().types.clone();
    let owned = EditorComponents { types: components };
    let registry = registry_arc.read();
    let mut subtrees = Vec::new();
    let (mut derived, mut lossy) = (0usize, 0usize);
    for (_, entity) in &found.subject {
        match crate::edits::copy_subtree(world, &registry, &owned, *entity) {
            Ok(subtree) => subtrees.push(subtree),
            Err(crate::edits::CopyRefusal::DerivedRoot) => derived += 1,
            Err(crate::edits::CopyRefusal::LosesContentUnderDerived) => lossy += 1,
            Err(crate::edits::CopyRefusal::Unnamed) => {}
        }
    }
    drop(registry);
    (found, subtrees, derived, lossy)
}
fn capture_selected(world: &mut World) -> Vec<(SceneId, Vec<Box<dyn PartialReflect>>)> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let components = world.resource::<EditorComponents>().types.clone();
    let mut selected: Vec<(Entity, SceneId)> = {
        let mut query = world.query_filtered::<(Entity, &SceneId), With<Selected>>();
        query.iter(world).map(|(e, id)| (e, *id)).collect()
    };
    selected.sort_by_key(|(_, id)| id.0);
    selected
        .into_iter()
        .map(|(entity, id)| {
            let mut captured = Vec::new();
            for reg in &components {
                let Some(registration) = registry.get(reg.type_id) else {
                    continue;
                };
                let Some(reflect_component) =
                    registration.data::<bevy::ecs::reflect::ReflectComponent>()
                else {
                    continue;
                };
                if let Some(value) = reflect_component.reflect(world.entity(entity)) {
                    captured.push(value.as_partial_reflect().to_dynamic());
                }
            }
            (id, captured)
        })
        .collect()
}

pub(crate) fn perform_clipboard(world: &mut World) {
    let requests = std::mem::take(&mut *world.resource_mut::<ClipboardRequests>());
    if !requests.cut && !requests.yank && !requests.paste && !requests.duplicate {
        return;
    }

    // Duplicate is NOT yank-then-paste: it must not clobber the register. A
    // designer yanks a piece, lays a run of duplicates, and still expects `p`
    // to paste what they yanked.
    if requests.duplicate {
        let (found, subtrees, derived, lossy) = capture_copy_set(world);
        if let Some(message) = crate::layout::refusal(&found, "duplicate") {
            say(world, message, false);
        } else if subtrees.is_empty() {
            say(
                world,
                copy_refusal_message("duplicate", derived, lossy),
                false,
            );
        } else {
            let mut ops: Vec<Op> = Vec::new();
            let mut roots: Vec<SceneId> = Vec::new();
            for subtree in &subtrees {
                let (mut made, root) = crate::edits::copy_ops(subtree, subtree.external_parent);
                ops.append(&mut made);
                roots.push(root);
            }
            world.resource_mut::<EditQueue>().0.push(Transaction {
                label: format!("Duplicate {}", subtrees.len()),
                gesture: None,
                ops,
            });
            // ROOTS only: a copied group is ONE thing to place, and handing
            // the gesture its children too would ask them to move themselves
            // as well as riding the root.
            world.resource_mut::<PendingPasteSelect>().0 = roots;
            // The copies land exactly on their originals, so handing them to a
            // move gesture is the difference between "something happened" and a
            // duplicate that looks like nothing at all. Esc leaves them in
            // place; the duplicate is its own transaction either way.
            world.resource_mut::<PendingDuplicateGrab>().0 = Some(1);
        }
    }

    if requests.yank || requests.cut {
        let captured = capture_selected(world);
        if !captured.is_empty() {
            world.resource_mut::<EditorClipboard>().entries = captured
                .iter()
                .map(|(_, c)| c.iter().map(|v| v.to_dynamic()).collect())
                .collect();
        }
        if requests.cut {
            let ids: Vec<SceneId> = captured.iter().map(|(id, _)| *id).collect();
            if !ids.is_empty() {
                let ops = ids
                    .into_iter()
                    .map(|id| Op::Despawn { id })
                    .collect::<Vec<_>>();
                let label = format!("Delete {}", ops.len());
                world.resource_mut::<EditQueue>().0.push(Transaction {
                    label,
                    gesture: None,
                    ops,
                });
            }
        }
    }

    if requests.paste {
        let entries: Vec<Vec<Box<dyn PartialReflect>>> = {
            let clipboard = world.resource::<EditorClipboard>();
            clipboard
                .entries
                .iter()
                .map(|entry| entry.iter().map(|v| v.to_dynamic()).collect())
                .collect()
        };
        if !entries.is_empty() {
            let mut new_ids = Vec::new();
            let ops = entries
                .into_iter()
                .map(|components| {
                    let id = SceneId::random();
                    new_ids.push(id);
                    Op::Spawn { id, components }
                })
                .collect::<Vec<_>>();
            let label = format!("Paste {}", ops.len());
            world.resource_mut::<EditQueue>().0.push(Transaction {
                label,
                gesture: None,
                ops,
            });
            world.resource_mut::<PendingPasteSelect>().0 = new_ids;
        }
    }
}

/// Hand a finished duplicate to the move gesture, one frame after its copies
/// were selected.
pub(crate) fn grab_duplicate(
    mut pending: ResMut<PendingDuplicateGrab>,
    selected: Query<(), With<Selected>>,
    mut actions: MessageWriter<ActionInvoked>,
) {
    let Some(frames) = pending.0 else { return };
    if frames > 0 {
        pending.0 = Some(frames - 1);
        return;
    }
    pending.0 = None;
    if selected.iter().next().is_none() {
        return; // nothing to move — the spawns never resolved
    }
    actions.write(ActionInvoked {
        action: ActionId::new_static("transform.move"),
        args: None,
        source: InvocationSource::Test,
    });
}

/// After the paste transaction applies (Mutate), select what was pasted.
pub(crate) fn select_pasted(
    mut pending: ResMut<PendingPasteSelect>,
    index: Res<SceneIndex>,
    previous: Query<Entity, With<Selected>>,
    mut changed: MessageWriter<SelectionChanged>,
    mut commands: Commands,
) {
    if pending.0.is_empty() {
        return;
    }
    let resolved: Vec<Entity> = pending.0.iter().filter_map(|id| index.get(id)).collect();
    if resolved.is_empty() {
        return; // spawns not applied yet — retry next frame
    }
    pending.0.clear();
    for entity in &previous {
        commands.entity(entity).remove::<Selected>();
    }
    for entity in resolved {
        commands.entity(entity).insert(Selected);
    }
    changed.write(SelectionChanged);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditorCorePlugin;
    use crate::edits::{History, HistoryRequests};

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component)]
    struct Payload(f32);

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("clip-test", "Clip Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Payload>();
            reg.component::<Transform>();
        }
    }

    fn invoke(app: &mut App, action: &str) {
        app.world_mut().write_message(ActionInvoked {
            action: ActionId::new(action.to_string()),
            args: None,
            source: InvocationSource::Test,
        });
        app.update();
    }

    fn payload_values(app: &mut App) -> Vec<f32> {
        let world = app.world_mut();
        let mut values: Vec<f32> = world.query::<&Payload>().iter(world).map(|p| p.0).collect();
        values.sort_by(f32::total_cmp);
        values
    }

    // d cuts (one undoable delete), p pastes copies (one undoable spawn, selected).

    fn duplicate_app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<bevy::input::ButtonInput<KeyCode>>();
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;
        app
    }

    fn spawn_selected_payload(app: &mut App, value: f32) -> SceneId {
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
                        Box::new(Payload(value)).into_partial_reflect(),
                        Box::new(Transform::from_xyz(1.0, 2.0, 3.0)).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);
        id
    }

    // Spec §2 registration surface names duplicate, and the v1 post-mortem
    // records ITS duplicate as lossy (kind + position + rotation only). This one
    // copies every registered component, in ONE undo entry.
    #[test]
    fn duplicate_copies_every_component_in_one_entry() {
        let mut app = duplicate_app();
        let original = spawn_selected_payload(&mut app, 7.0);
        let depth_before = app.world().resource::<History>().undo_depth();

        invoke(&mut app, "select.duplicate");
        app.update();

        assert_eq!(
            payload_values(&mut app),
            vec![7.0, 7.0],
            "the copy carries the payload, not just a transform"
        );
        let transforms: Vec<Vec3> = {
            let world = app.world_mut();
            world
                .query::<&Transform>()
                .iter(world)
                .map(|t| t.translation)
                .collect()
        };
        assert_eq!(
            transforms,
            vec![Vec3::new(1.0, 2.0, 3.0); 2],
            "and lands exactly on the original, for the grab to move"
        );
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth_before + 1,
            "one duplicate is ONE undo entry"
        );
        let _ = original;
    }

    // The copies become the selection: the next gesture moves what you just
    // made, not what you made it from.
    #[test]
    fn duplicate_selects_the_copies() {
        let mut app = duplicate_app();
        let original = spawn_selected_payload(&mut app, 1.0);
        invoke(&mut app, "select.duplicate");
        app.update();
        app.update();

        let original_entity = app.world().resource::<SceneIndex>().get(&original).unwrap();
        assert!(
            app.world().get::<Selected>(original_entity).is_none(),
            "the original is deselected"
        );
        let selected = {
            let world = app.world_mut();
            world
                .query_filtered::<(), With<Selected>>()
                .iter(world)
                .count()
        };
        assert_eq!(selected, 1, "exactly the copy is selected");
    }

    // A duplicate must not clobber the register: yank a piece, lay a run of
    // duplicates, and `p` still pastes what was yanked.
    #[test]
    fn duplicate_leaves_the_yank_register_alone() {
        let mut app = duplicate_app();
        let _ = spawn_selected_payload(&mut app, 5.0);
        invoke(&mut app, "select.yank");
        app.update();

        // Select something else and duplicate it.
        let other = spawn_selected_payload(&mut app, 9.0);
        {
            let world = app.world_mut();
            let keep = world.resource::<SceneIndex>().get(&other).unwrap();
            let stale: Vec<Entity> = world
                .query_filtered::<Entity, With<Selected>>()
                .iter(world)
                .filter(|entity| *entity != keep)
                .collect();
            for entity in stale {
                world.entity_mut(entity).remove::<Selected>();
            }
        }
        invoke(&mut app, "select.duplicate");
        app.update();

        let register: Vec<f32> = {
            let clipboard = app.world().resource::<EditorClipboard>();
            clipboard
                .entries
                .iter()
                .filter_map(|entry| {
                    entry
                        .iter()
                        .find_map(|value| Payload::from_reflect(value.as_ref()))
                })
                .map(|payload| payload.0)
                .collect()
        };
        assert_eq!(
            register,
            vec![5.0],
            "the register still holds the yank, not the duplicate"
        );
    }
    #[test]
    fn cut_paste_round_trip() {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<bevy::input::ButtonInput<KeyCode>>();
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;

        for value in [1.0_f32, 2.0] {
            app.world_mut()
                .resource_mut::<EditQueue>()
                .0
                .push(Transaction {
                    label: "spawn".into(),
                    gesture: None,
                    ops: vec![Op::Spawn {
                        id: SceneId::random(),
                        components: vec![Box::new(Payload(value)).into_partial_reflect()],
                    }],
                });
        }
        app.update();
        let world = app.world_mut();
        let entities: Vec<Entity> = world
            .query_filtered::<Entity, With<SceneId>>()
            .iter(world)
            .collect();
        for entity in entities {
            world.entity_mut(entity).insert(Selected);
        }

        invoke(&mut app, "select.delete");
        assert_eq!(
            payload_values(&mut app),
            Vec::<f32>::new(),
            "cut removed both"
        );

        // Undo the cut restores them.
        app.world_mut().resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        assert_eq!(payload_values(&mut app), vec![1.0, 2.0]);

        // Paste adds copies and selects them.
        let depth = app.world().resource::<History>().undo_depth();
        invoke(&mut app, "select.paste");
        app.update();
        assert_eq!(payload_values(&mut app), vec![1.0, 1.0, 2.0, 2.0]);
        assert_eq!(app.world().resource::<History>().undo_depth(), depth + 1);
        let world = app.world_mut();
        let selected = world
            .query_filtered::<(), With<Selected>>()
            .iter(world)
            .count();
        assert_eq!(selected, 2, "pasted entities are the new selection");
    }

    /// a → b → c, with only `a` selected.
    fn spawn_selected_group(app: &mut App) -> (SceneId, SceneId, SceneId) {
        let (a, b, c) = (SceneId::random(), SceneId::random(), SceneId::random());
        let payload = |v: f32| vec![Box::new(Payload(v)).into_partial_reflect()];
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "spawn group".into(),
                gesture: None,
                ops: vec![
                    Op::Spawn {
                        id: a,
                        components: payload(1.0),
                    },
                    Op::Spawn {
                        id: b,
                        components: payload(2.0),
                    },
                    Op::Spawn {
                        id: c,
                        components: payload(3.0),
                    },
                    Op::Reparent {
                        target: b,
                        parent: Some(a),
                    },
                    Op::Reparent {
                        target: c,
                        parent: Some(b),
                    },
                ],
            });
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&a).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);
        (a, b, c)
    }

    fn scene_count(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<(), With<SceneId>>()
            .iter(app.world())
            .count()
    }

    fn drain_feedback(app: &mut App) -> Vec<String> {
        let messages = app
            .world()
            .resource::<bevy::ecs::message::Messages<editor_api::feedback::SceneIoFeedback>>();
        let mut cursor = messages.get_cursor();
        cursor.read(messages).map(|m| m.message.clone()).collect()
    }

    /// A copied group lands as a GROUP. Before this it landed as a childless
    /// root and the children were simply not there.
    #[test]
    fn duplicating_a_two_level_group_lands_as_a_group() {
        let mut app = duplicate_app();
        let (a, _, _) = spawn_selected_group(&mut app);
        let depth = app.world().resource::<History>().undo_depth();
        invoke(&mut app, "select.duplicate");
        app.update();

        assert_eq!(scene_count(&mut app), 6, "the copy came without its family");
        let original = app.world().resource::<SceneIndex>().get(&a).unwrap();
        let copy = app
            .world_mut()
            .query_filtered::<Entity, (With<SceneId>, Without<ChildOf>)>()
            .iter(app.world())
            .find(|e| *e != original)
            .expect("no copied root");
        let child = app
            .world()
            .get::<Children>(copy)
            .expect("the copy has no children")
            .iter()
            .next()
            .unwrap();
        let grandchild = app
            .world()
            .get::<Children>(child)
            .expect("the copy is only one level deep")
            .iter()
            .next()
            .unwrap();
        // The grandchild hangs under the COPY's child, not the original's.
        assert_eq!(
            app.world().get::<ChildOf>(grandchild).unwrap().parent(),
            child
        );
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth + 1,
            "a duplicated group must be ONE undo entry"
        );
        app.world_mut().resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        assert_eq!(scene_count(&mut app), 3, "undo left husks behind");
    }

    /// Selecting a parent AND its child copies the child ONCE. Without the fold
    /// it is copied twice — once inside the subtree and once on its own.
    #[test]
    fn duplicating_a_parent_and_its_selected_child_copies_the_child_once() {
        let mut app = duplicate_app();
        let (_, b, _) = spawn_selected_group(&mut app);
        let entity = app.world().resource::<SceneIndex>().get(&b).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);
        invoke(&mut app, "select.duplicate");
        app.update();
        assert_eq!(scene_count(&mut app), 6, "the child was copied twice");
    }

    /// A copy of a child stays inside its own parent, rather than silently
    /// detaching to the world root.
    #[test]
    fn a_duplicate_stays_inside_its_own_parent() {
        let mut app = duplicate_app();
        let (a, b, _) = spawn_selected_group(&mut app);
        let a_entity = app.world().resource::<SceneIndex>().get(&a).unwrap();
        app.world_mut().entity_mut(a_entity).remove::<Selected>();
        let b_entity = app.world().resource::<SceneIndex>().get(&b).unwrap();
        app.world_mut().entity_mut(b_entity).insert(Selected);
        invoke(&mut app, "select.duplicate");
        app.update();

        let copies: Vec<Entity> = app
            .world_mut()
            .query_filtered::<Entity, With<SceneId>>()
            .iter(app.world())
            .collect();
        let new_child = copies
            .into_iter()
            .filter(|e| *e != b_entity)
            .find(|e| {
                app.world()
                    .get::<ChildOf>(*e)
                    .is_some_and(|c| c.parent() == a_entity)
            })
            .expect("the copy detached from its parent");
        assert_ne!(new_child, b_entity);
    }

    /// Duplicating a locked object used to spawn a locked copy sitting
    /// invisibly on its original that could never be moved. It refuses now,
    /// and says how to proceed.
    #[test]
    fn duplicating_a_locked_object_refuses_and_says_so() {
        let mut app = duplicate_app();
        let (a, _, _) = spawn_selected_group(&mut app);
        let entity = app.world().resource::<SceneIndex>().get(&a).unwrap();
        app.world_mut()
            .entity_mut(entity)
            .insert(crate::lock::Locked);
        invoke(&mut app, "select.duplicate");
        app.update();
        assert_eq!(scene_count(&mut app), 3, "a locked object was duplicated");
        let said = drain_feedback(&mut app);
        assert!(
            said.iter().any(|m| m.contains("\u{2423}l")),
            "the refusal did not name the way out: {said:?}"
        );
    }

    /// The copies are handed to a move gesture, and a group is ONE thing to
    /// place — handing it its children too would ask them to move themselves
    /// as well as riding the root.
    #[test]
    fn duplicate_selects_only_the_copied_roots() {
        let mut app = duplicate_app();
        let (_a, _, _) = spawn_selected_group(&mut app);
        invoke(&mut app, "select.duplicate");
        app.update();
        app.update();
        let selected = app
            .world_mut()
            .query_filtered::<(), With<Selected>>()
            .iter(app.world())
            .count();
        assert_eq!(selected, 1, "the copied children were selected too");
    }
}
