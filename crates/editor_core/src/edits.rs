//! The kernel's edit engine (RFC §5, M2): drains `EditQueue` at `EditorSet::Mutate`,
//! applies ops via reflection, captures inverses generically, records history with
//! gesture coalescing under the FIRST-old-value contract (spike finding F1), and
//! services undo/redo. The only code in the editor that mutates scene components.

use bevy::ecs::lifecycle::{Add, Remove};
use bevy::ecs::relationship::RelationshipHookMode;
use bevy::prelude::*;
use bevy::reflect::{PartialReflect, TypeRegistry};
use editor_api::feature::ComponentReg;
use editor_api::prelude::*;

/// Registered editor components (from feature registration) — the capture set for
/// despawn inverses and, later, serialization.
#[derive(Resource, Default)]
pub struct EditorComponents {
    pub types: Vec<ComponentReg>,
}

fn noop_register(_: &bevy::ecs::reflect::AppTypeRegistry) {}

impl EditorComponents {
    pub fn contains(&self, type_id: std::any::TypeId) -> bool {
        self.types.iter().any(|r| r.type_id == type_id)
    }
    /// Adopt a type into the save allow-list at runtime (owner rule: anything
    /// a user INSERTS must persist; only system-derived state stays out).
    /// The type must already live in the `AppTypeRegistry`.
    pub fn adopt(&mut self, type_id: std::any::TypeId, type_path: &'static str) {
        if !self.contains(type_id) {
            self.types.push(editor_api::feature::ComponentReg {
                type_id,
                type_path,
                register: noop_register,
            });
        }
    }
}

pub struct HistoryEntry {
    pub label: String,
    pub gesture: Option<u64>,
    inverse: Vec<Op>,
}

#[derive(Resource, Default)]
pub struct History {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

impl History {
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }
    pub fn redo_depth(&self) -> usize {
        self.redo.len()
    }
    pub fn undo_labels(&self) -> impl Iterator<Item = &str> {
        self.undo.iter().rev().map(|e| e.label.as_str())
    }
    /// Loading/restoring a scene invalidates history wholesale.
    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }
}

/// Batch-coalescing hook: when armed, all history entries created THIS frame merge
/// into one undoable step. Retained for scripted/batch invocations (palette scripts,
/// future macros — deferred per M2, see docs/M2-ACCEPTANCE.md B11).
#[derive(Resource, Default)]
pub struct MergeFrameEntries(pub bool);

/// Who owns Ctrl+Z right now (M4-D11 asset history): the SCENE history by
/// default; an open asset editor (materials) claims the scope so undo targets
/// the asset being edited, never silently unwinding scene work behind it.
#[derive(Resource, Default, PartialEq, Eq, Clone, Copy)]
pub enum HistoryScope {
    #[default]
    Scene,
    Asset,
}

/// Undo/redo requests, set by the action handler (Tools), consumed at Mutate.
#[derive(Resource, Default)]
pub struct HistoryRequests {
    pub undo: usize,
    pub redo: usize,
    /// Cancel an in-flight gesture: pop its (single, coalesced) entry and apply the
    /// inverse WITHOUT creating a redo — Esc-cancel restores originals exactly (B7).
    pub cancel_gesture: Option<u64>,
}

/// Keep the `SceneId → Entity` index true via lifecycle observers.
pub(crate) fn index_on_add(
    add: On<Add, SceneId>,
    ids: Query<&SceneId>,
    mut index: ResMut<SceneIndex>,
) {
    let entity = add.entity;
    if let Ok(id) = ids.get(entity) {
        index.insert(*id, entity);
    }
}

pub(crate) fn index_on_remove(
    remove: On<Remove, SceneId>,
    ids: Query<&SceneId>,
    mut index: ResMut<SceneIndex>,
) {
    let entity = remove.entity;
    if let Ok(id) = ids.get(entity) {
        index.remove(id);
    }
}

/// Editor invariant (owner): every scene entity HAS a Name. Anything that arrives
/// without one (legacy scenes, direct spawns, pastes of old data) gets a generated
/// one. Direct insert, not a transaction — normalization must not pollute history;
/// Name is a registered component, so it serializes from then on.
pub(crate) fn ensure_entity_names(
    unnamed: Query<(Entity, &SceneId), Without<Name>>,
    mut commands: Commands,
) {
    for (entity, id) in &unnamed {
        // try_insert: a same-frame restamp/close may despawn the entity after
        // this queues (stamped children without a template Name) — never panic.
        commands
            .entity(entity)
            .try_insert(Name::new(format!("Entity {}", &id.0.to_string()[..4])));
    }
}

/// Consume undo/redo actions (any invocation source) into requests.
pub(crate) fn handle_history_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<crate::resolver::EditorState>,
    scope: Res<HistoryScope>,
    mut requests: ResMut<HistoryRequests>,
) {
    for invoked in reader.read() {
        // Global bindings, editor-gated semantics: undo/redo never fire while the
        // game owns input (play sessions must not eat the history), and an open
        // asset editor owns them while it holds the scope.
        if !state.active || *scope == HistoryScope::Asset {
            continue;
        }
        match invoked.action.as_str() {
            "core.undo" => requests.undo += 1,
            "core.redo" => requests.redo += 1,
            _ => {}
        }
    }
}

fn reflect_component_for<'r>(
    registry: &'r TypeRegistry,
    value: &dyn PartialReflect,
) -> Option<(&'r bevy::ecs::reflect::ReflectComponent, &'static str)> {
    let info = value.get_represented_type_info()?;
    let registration = registry.get(info.type_id())?;
    Some((
        registration.data::<bevy::ecs::reflect::ReflectComponent>()?,
        info.type_path(),
    ))
}

fn clone_component(
    world: &World,
    registry: &TypeRegistry,
    entity: Entity,
    type_id: std::any::TypeId,
) -> Option<Box<dyn PartialReflect>> {
    let reflect_component = registry
        .get(type_id)?
        .data::<bevy::ecs::reflect::ReflectComponent>()?;
    let entity_ref = world.get_entity(entity).ok()?;
    Some(reflect_component.reflect(entity_ref)?.to_dynamic())
}

/// Apply one op, returning its inverse (None if the op degenerated to a no-op).
fn apply_op(
    world: &mut World,
    registry: &TypeRegistry,
    editor_components: &EditorComponents,
    op: Op,
    touched: &mut Vec<SceneId>,
) -> Option<Op> {
    let resolve = |world: &World, id: &SceneId| world.resource::<SceneIndex>().get(id);
    match op {
        Op::Set { target, value } => {
            let entity = resolve(world, &target)?;
            let (reflect_component, type_path) = reflect_component_for(registry, value.as_ref())?;
            let type_id = value.get_represented_type_info()?.type_id();
            let inverse = match clone_component(world, registry, entity, type_id) {
                Some(old) => Op::Set { target, value: old },
                None => Op::Remove {
                    target,
                    type_path: type_path.to_string(),
                },
            };
            let mut entity_mut = world.get_entity_mut(entity).ok()?;
            reflect_component.apply_or_insert_mapped(
                &mut entity_mut,
                value.as_ref(),
                registry,
                &mut (),
                RelationshipHookMode::Run,
            );
            touched.push(target);
            Some(inverse)
        }
        Op::Patch {
            target,
            type_path,
            path,
            value,
        } => {
            let entity = resolve(world, &target)?;
            let registration = registry.get_with_type_path(&type_path)?;
            let reflect_component = registration.data::<bevy::ecs::reflect::ReflectComponent>()?;
            // The component still round-trips as a whole — that is how a
            // reflected component is written back — but the OP carries one leaf,
            // so the history entry is a field and the inverse is the field's
            // previous value.
            let mut dynamic = clone_component(world, registry, entity, registration.type_id())?;
            let parsed = bevy::reflect::ParsedPath::parse(&path).ok()?;
            let element = parsed.reflect_element_mut(dynamic.as_mut()).ok()?;
            let old = element.to_dynamic();
            element.try_apply(value.as_ref()).ok()?;
            let mut entity_mut = world.get_entity_mut(entity).ok()?;
            reflect_component.apply_or_insert_mapped(
                &mut entity_mut,
                dynamic.as_ref(),
                registry,
                &mut (),
                RelationshipHookMode::Run,
            );
            touched.push(target);
            Some(Op::Patch {
                target,
                type_path,
                path,
                value: old,
            })
        }
        Op::Remove { target, type_path } => {
            let entity = resolve(world, &target)?;
            let registration = registry.get_with_type_path(&type_path)?;
            let reflect_component = registration.data::<bevy::ecs::reflect::ReflectComponent>()?;
            let old = clone_component(world, registry, entity, registration.type_id())?;
            let mut entity_mut = world.get_entity_mut(entity).ok()?;
            reflect_component.remove(&mut entity_mut);
            touched.push(target);
            Some(Op::Set { target, value: old })
        }
        Op::Spawn { id, components } => {
            let entity = world.spawn(id).id();
            for value in components {
                let Some((reflect_component, _)) = reflect_component_for(registry, value.as_ref())
                else {
                    continue;
                };
                let mut entity_mut = world.get_entity_mut(entity).ok()?;
                reflect_component.apply_or_insert_mapped(
                    &mut entity_mut,
                    value.as_ref(),
                    registry,
                    &mut (),
                    RelationshipHookMode::Run,
                );
            }
            touched.push(id);
            Some(Op::Despawn { id })
        }
        Op::Despawn { id } => {
            let entity = match resolve(world, &id) {
                Some(entity) => entity,
                None => {
                    // A despawn that silently no-ops leaves ghosts behind —
                    // this is always a caller bug worth hearing about.
                    warn!("Op::Despawn target {id:?} not in SceneIndex — skipped");
                    return None;
                }
            };
            let mut captured = Vec::new();
            for reg in &editor_components.types {
                if let Some(value) = clone_component(world, registry, entity, reg.type_id) {
                    captured.push(value);
                }
            }
            world.entity_mut(entity).despawn();
            touched.push(id);
            Some(Op::Spawn {
                id,
                components: captured,
            })
        }
        Op::Reparent { target, parent } => {
            let entity = resolve(world, &target)?;
            let old_parent = world
                .get::<ChildOf>(entity)
                .and_then(|c| world.get::<SceneId>(c.parent()))
                .copied();
            match parent {
                Some(parent_id) => {
                    let parent_entity = resolve(world, &parent_id)?;
                    world.entity_mut(entity).insert(ChildOf(parent_entity));
                }
                None => {
                    world.entity_mut(entity).remove::<ChildOf>();
                }
            }
            touched.push(target);
            Some(Op::Reparent {
                target,
                parent: old_parent,
            })
        }
    }
}

fn apply_ops(
    world: &mut World,
    registry: &TypeRegistry,
    editor_components: &EditorComponents,
    ops: Vec<Op>,
    touched: &mut Vec<SceneId>,
) -> Vec<Op> {
    let mut inverse = Vec::with_capacity(ops.len());
    for op in ops {
        if let Some(inv) = apply_op(world, registry, editor_components, op, touched) {
            inverse.push(inv);
        }
    }
    inverse.reverse();
    inverse
}

/// THE mutation point (`EditorSet::Mutate`, exclusive).
pub fn apply_edits(world: &mut World) {
    let queue = std::mem::take(&mut world.resource_mut::<EditQueue>().0);
    let requests = std::mem::take(&mut *world.resource_mut::<HistoryRequests>());
    if queue.is_empty()
        && requests.undo == 0
        && requests.redo == 0
        && requests.cancel_gesture.is_none()
    {
        return;
    }

    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    let editor_components = std::mem::take(&mut *world.resource_mut::<EditorComponents>());
    let mut touched: Vec<SceneId> = Vec::new();
    let depth_before_queue = world.resource::<History>().undo.len();

    for mut transaction in queue {
        // THE LOCK, enforced once. Every mutation in this editor arrives here,
        // so a guard on the queue covers every verb — including the ones written
        // after it — instead of a check each verb has to remember. Refused ops
        // are dropped from the transaction; the rest of it still applies, so
        // moving ten objects with two locked moves the eight.
        let refused = {
            let index = world.resource::<SceneIndex>();
            let locked: std::collections::HashSet<SceneId> = index
                .iter()
                .filter(|(_, entity)| world.get::<crate::lock::Locked>(**entity).is_some())
                .map(|(id, _)| *id)
                .collect();
            if locked.is_empty() {
                0
            } else {
                let before = transaction.ops.len();
                transaction
                    .ops
                    .retain(|op| !crate::lock::op_is_refused(op, |id| locked.contains(&id)));
                before - transaction.ops.len()
            }
        };
        if refused > 0 && transaction.ops.is_empty() {
            // Say so, in the statusbar and not the log: a verb that silently
            // does nothing reads as a broken editor, and "it is locked" is the
            // whole answer. Every frame of a drag re-says it, which is right —
            // the message should stay lit for as long as you keep trying.
            world.write_message(editor_api::feedback::SceneIoFeedback {
                message: format!(
                    "{refused} locked object{} \u{00b7} \u{2423}l to unlock",
                    if refused == 1 { "" } else { "s" }
                ),
                success: false,
            });
            continue;
        }
        let inverse = apply_ops(
            world,
            &registry,
            &editor_components,
            transaction.ops,
            &mut touched,
        );
        if inverse.is_empty() {
            continue;
        }
        let mut history = world.resource_mut::<History>();
        history.redo.clear();
        // Gesture coalescing, FIRST-old-value contract (F1): same gesture ⇒ the
        // original entry's inverse already restores to pre-gesture state; the new
        // forward ops are applied but their inverses are DISCARDED.
        let coalesce = transaction.gesture.is_some()
            && history
                .undo
                .last()
                .is_some_and(|e| e.gesture == transaction.gesture);
        if !coalesce {
            history.undo.push(HistoryEntry {
                label: transaction.label,
                gesture: transaction.gesture,
                inverse,
            });
        }
    }

    if let Some(gesture) = requests.cancel_gesture {
        let matches = world
            .resource::<History>()
            .undo
            .last()
            .is_some_and(|e| e.gesture == Some(gesture));
        if matches {
            let entry = world.resource_mut::<History>().undo.pop().unwrap();
            apply_ops(
                world,
                &registry,
                &editor_components,
                entry.inverse,
                &mut touched,
            );
        }
    }

    // Macro replay: merge every entry the queue just produced into one step.
    if std::mem::take(&mut world.resource_mut::<MergeFrameEntries>().0) {
        let mut history = world.resource_mut::<History>();
        if history.undo.len() > depth_before_queue + 1 {
            let tail: Vec<HistoryEntry> = history.undo.split_off(depth_before_queue);
            let mut inverse = Vec::new();
            for entry in tail.into_iter().rev() {
                inverse.extend(entry.inverse);
            }
            history.undo.push(HistoryEntry {
                label: "Replay macro".to_string(),
                gesture: None,
                inverse,
            });
        }
    }

    for _ in 0..requests.undo {
        let Some(entry) = world.resource_mut::<History>().undo.pop() else {
            break;
        };
        let redo_ops = apply_ops(
            world,
            &registry,
            &editor_components,
            entry.inverse,
            &mut touched,
        );
        world.resource_mut::<History>().redo.push(HistoryEntry {
            label: entry.label,
            gesture: None,
            inverse: redo_ops,
        });
    }
    for _ in 0..requests.redo {
        let Some(entry) = world.resource_mut::<History>().redo.pop() else {
            break;
        };
        let undo_ops = apply_ops(
            world,
            &registry,
            &editor_components,
            entry.inverse,
            &mut touched,
        );
        world.resource_mut::<History>().undo.push(HistoryEntry {
            label: entry.label,
            gesture: None,
            inverse: undo_ops,
        });
    }

    *world.resource_mut::<EditorComponents>() = editor_components;
    if !touched.is_empty() {
        touched.sort_by_key(|id| id.0);
        touched.dedup();
        world.write_message(Edited { targets: touched });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EditorCorePlugin;

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component)]
    struct Health {
        current: f32,
        max: f32,
    }

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("test", "Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Health>()
                .component::<Transform>()
                .action(ActionDef::new("test.heal", "Heal").edit());
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.finish();
        app.update();
        app
    }

    fn edit(app: &mut App, f: impl FnOnce(&mut EditQueue)) {
        f(&mut app.world_mut().resource_mut::<EditQueue>());
        app.update();
    }

    fn snapshot(app: &mut App) -> Vec<(SceneId, Option<Health>, Option<Vec3>)> {
        let world = app.world_mut();
        let mut rows: Vec<_> = world
            .query::<(&SceneId, Option<&Health>, Option<&Transform>)>()
            .iter(world)
            .map(|(id, h, t)| (*id, h.cloned(), t.map(|t| t.translation)))
            .collect();
        rows.sort_by_key(|(id, ..)| id.0);
        rows
    }

    fn undo(app: &mut App, n: usize) {
        app.world_mut().resource_mut::<HistoryRequests>().undo = n;
        app.update();
    }
    fn redo(app: &mut App, n: usize) {
        app.world_mut().resource_mut::<HistoryRequests>().redo = n;
        app.update();
    }

    // B1: spawn/set/remove/despawn/reparent round-trip through the queue
    #[test]
    fn ops_apply_and_index_tracks() {
        let mut app = test_app();
        let a = SceneId::random();
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: a,
                    components: vec![
                        Box::new(Health {
                            current: 10.0,
                            max: 10.0,
                        })
                        .into_partial_reflect(),
                    ],
                }],
            });
        });
        assert_eq!(app.world_mut().resource::<SceneIndex>().len(), 1);
        assert_eq!(
            snapshot(&mut app)[0].1,
            Some(Health {
                current: 10.0,
                max: 10.0
            })
        );

        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "set".into(),
                gesture: None,
                ops: vec![Op::Set {
                    target: a,
                    value: Box::new(Health {
                        current: 3.0,
                        max: 10.0,
                    })
                    .into_partial_reflect(),
                }],
            });
        });
        assert_eq!(
            snapshot(&mut app)[0].1,
            Some(Health {
                current: 3.0,
                max: 10.0
            })
        );

        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "despawn".into(),
                gesture: None,
                ops: vec![Op::Despawn { id: a }],
            });
        });
        assert!(snapshot(&mut app).is_empty());
        assert_eq!(app.world_mut().resource::<SceneIndex>().len(), 0);
    }

    // B2: undo-all returns to initial; redo-all returns to final — exactly.
    #[test]
    fn undo_redo_round_trip() {
        let mut app = test_app();
        let a = SceneId::random();
        let b = SceneId::random();
        let initial = snapshot(&mut app);

        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "spawn a".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: a,
                    components: vec![
                        Box::new(Health {
                            current: 5.0,
                            max: 9.0,
                        })
                        .into_partial_reflect(),
                        Box::new(Transform::from_xyz(1.0, 2.0, 3.0)).into_partial_reflect(),
                    ],
                }],
            });
        });
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "spawn b + edit a".into(),
                gesture: None,
                ops: vec![
                    Op::Spawn {
                        id: b,
                        components: vec![],
                    },
                    Op::Set {
                        target: a,
                        value: Box::new(Health {
                            current: 1.0,
                            max: 9.0,
                        })
                        .into_partial_reflect(),
                    },
                ],
            });
        });
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "remove health".into(),
                gesture: None,
                ops: vec![Op::Remove {
                    target: a,
                    type_path: std::any::type_name::<Health>().to_string(),
                }],
            });
        });
        let fin = snapshot(&mut app);
        assert_eq!(app.world().resource::<History>().undo_depth(), 3);

        undo(&mut app, 3);
        assert_eq!(snapshot(&mut app), initial, "undo-all must restore initial");
        redo(&mut app, 3);
        assert_eq!(snapshot(&mut app), fin, "redo-all must restore final");
        undo(&mut app, 1);
        let row_a = snapshot(&mut app)
            .into_iter()
            .find(|(id, ..)| *id == a)
            .unwrap();
        assert_eq!(
            row_a.1,
            Some(Health {
                current: 1.0,
                max: 9.0
            })
        );
    }

    // Spec §5: patches are THE one delta language. A patch addresses one leaf by
    // reflect path, and its inverse carries that leaf's previous value — so the
    // history entry for a slider drag is an f32, not a whole Transform.
    #[test]
    fn a_patch_sets_one_leaf_and_inverts_to_the_old_one() {
        let mut app = test_app();
        let id = SceneId::random();
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id,
                    components: vec![
                        Box::new(Transform::from_xyz(1.0, 2.0, 3.0)).into_partial_reflect(),
                    ],
                }],
            });
        });
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "edit y".into(),
                gesture: None,
                ops: vec![Op::Patch {
                    target: id,
                    type_path: "bevy_transform::components::transform::Transform".into(),
                    path: "translation.y".into(),
                    value: Box::new(9.0_f32).into_partial_reflect(),
                }],
            });
        });
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        assert_eq!(
            app.world().get::<Transform>(entity).unwrap().translation,
            Vec3::new(1.0, 9.0, 3.0),
            "only the addressed leaf moved"
        );

        undo(&mut app, 1);
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        assert_eq!(
            app.world().get::<Transform>(entity).unwrap().translation,
            Vec3::new(1.0, 2.0, 3.0),
            "the inverse restored the leaf, leaving its siblings alone"
        );
    }

    // The whole point of field granularity: a patch must not clobber the fields
    // it did not address, even when something else changed them in between.
    #[test]
    fn a_patch_leaves_its_siblings_alone() {
        let mut app = test_app();
        let id = SceneId::random();
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id,
                    components: vec![
                        Box::new(Transform::from_xyz(0.0, 0.0, 0.0)).into_partial_reflect(),
                    ],
                }],
            });
        });
        for (path, value) in [("translation.x", 5.0_f32), ("translation.z", 7.0)] {
            edit(&mut app, |q| {
                q.0.push(Transaction {
                    label: "edit".into(),
                    gesture: None,
                    ops: vec![Op::Patch {
                        target: id,
                        type_path: "bevy_transform::components::transform::Transform".into(),
                        path: path.into(),
                        value: Box::new(value).into_partial_reflect(),
                    }],
                });
            });
        }
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        assert_eq!(
            app.world().get::<Transform>(entity).unwrap().translation,
            Vec3::new(5.0, 0.0, 7.0),
            "two patches to different leaves compose"
        );
    }

    // Coalescing is op-agnostic, and has to stay that way: thirty patched frames
    // of a drag are ONE entry that undoes to where the drag started.
    #[test]
    fn patched_gesture_frames_coalesce_to_one_entry() {
        let mut app = test_app();
        let id = SceneId::random();
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id,
                    components: vec![
                        Box::new(Transform::from_xyz(0.0, 0.0, 0.0)).into_partial_reflect(),
                    ],
                }],
            });
        });
        for i in 1..=30 {
            edit(&mut app, |q| {
                q.0.push(Transaction {
                    label: "drag".into(),
                    gesture: Some(11),
                    ops: vec![Op::Patch {
                        target: id,
                        type_path: "bevy_transform::components::transform::Transform".into(),
                        path: "translation.x".into(),
                        value: Box::new(i as f32 * 0.1).into_partial_reflect(),
                    }],
                });
            });
        }
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            2,
            "spawn + ONE gesture, however many patched frames"
        );
        undo(&mut app, 1);
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        assert_eq!(
            app.world().get::<Transform>(entity).unwrap().translation.x,
            0.0,
            "undo returns to where the drag STARTED (first-old-value)"
        );
    }

    // An unresolvable path is skipped rather than applied wrongly — the kernel
    // is the one place that validates, so the UI cannot smuggle a bad delta in.
    #[test]
    fn a_patch_with_a_bad_path_changes_nothing() {
        let mut app = test_app();
        let id = SceneId::random();
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id,
                    components: vec![
                        Box::new(Transform::from_xyz(1.0, 2.0, 3.0)).into_partial_reflect(),
                    ],
                }],
            });
        });
        let depth = app.world().resource::<History>().undo_depth();
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "bad".into(),
                gesture: None,
                ops: vec![Op::Patch {
                    target: id,
                    type_path: "bevy_transform::components::transform::Transform".into(),
                    path: "nonexistent.field".into(),
                    value: Box::new(9.0_f32).into_partial_reflect(),
                }],
            });
        });
        let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
        assert_eq!(
            app.world().get::<Transform>(entity).unwrap().translation,
            Vec3::new(1.0, 2.0, 3.0),
            "nothing moved"
        );
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth,
            "and nothing was recorded to undo"
        );
    }
    // F1 contract: a coalesced gesture is ONE entry and undo restores pre-gesture.
    #[test]
    fn gesture_coalesces_first_old_value() {
        let mut app = test_app();
        let a = SceneId::random();
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: a,
                    components: vec![
                        Box::new(Transform::from_xyz(0.0, 0.0, 0.0)).into_partial_reflect(),
                    ],
                }],
            });
        });
        for i in 1..=30 {
            edit(&mut app, |q| {
                q.0.push(Transaction {
                    label: "drag".into(),
                    gesture: Some(7),
                    ops: vec![Op::Set {
                        target: a,
                        value: Box::new(Transform::from_xyz(i as f32 * 0.1, 0.0, 0.0))
                            .into_partial_reflect(),
                    }],
                });
            });
        }
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            2,
            "spawn + ONE gesture"
        );
        undo(&mut app, 1);
        assert_eq!(
            snapshot(&mut app)[0].2,
            Some(Vec3::ZERO),
            "undo restores exact pre-gesture transform (first-old-value)"
        );
    }

    // B2 (reparent leg)
    #[test]
    fn reparent_round_trips() {
        let mut app = test_app();
        let parent = SceneId::random();
        let child = SceneId::random();
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![
                    Op::Spawn {
                        id: parent,
                        components: vec![],
                    },
                    Op::Spawn {
                        id: child,
                        components: vec![],
                    },
                ],
            });
        });
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "reparent".into(),
                gesture: None,
                ops: vec![Op::Reparent {
                    target: child,
                    parent: Some(parent),
                }],
            });
        });
        let world = app.world_mut();
        let child_entity = world.resource::<SceneIndex>().get(&child).unwrap();
        let parent_entity = world.resource::<SceneIndex>().get(&parent).unwrap();
        assert_eq!(
            world.get::<ChildOf>(child_entity).map(|c| c.parent()),
            Some(parent_entity)
        );
        undo(&mut app, 1);
        let world = app.world_mut();
        let child_entity = world.resource::<SceneIndex>().get(&child).unwrap();
        assert!(world.get::<ChildOf>(child_entity).is_none());
    }

    /// THE LOCK, end to end (owner: "lock objects to prevent further editing").
    ///
    /// The point is not that a predicate returns true — it is that the ONE
    /// place mutations happen honours it, and honours it PER OP: a transaction
    /// that touches a locked object and an unlocked one moves the unlocked one
    /// rather than failing whole. A lock that cancelled the batch would make
    /// locking a floor mean "you can never box-select again".
    #[test]
    fn locked_objects_refuse_edits_while_their_neighbours_move() {
        let mut app = test_app();
        let (free, held) = (SceneId::random(), SceneId::random());
        for id in [free, held] {
            edit(&mut app, |q| {
                q.0.push(Transaction {
                    label: "spawn".into(),
                    gesture: None,
                    ops: vec![Op::Spawn {
                        id,
                        components: vec![Box::new(Transform::default()).into_partial_reflect()],
                    }],
                });
            });
        }
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "lock".into(),
                gesture: None,
                ops: vec![Op::Set {
                    target: held,
                    value: Box::new(crate::lock::Locked).into_partial_reflect(),
                }],
            });
        });

        let moved = Transform::from_xyz(5.0, 0.0, 0.0);
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "move both".into(),
                gesture: None,
                ops: vec![
                    Op::Set {
                        target: free,
                        value: Box::new(moved).into_partial_reflect(),
                    },
                    Op::Set {
                        target: held,
                        value: Box::new(moved).into_partial_reflect(),
                    },
                ],
            });
        });
        let at = |app: &mut App, id: SceneId| {
            let world = app.world_mut();
            let entity = world.resource::<SceneIndex>().get(&id).unwrap();
            world.get::<Transform>(entity).unwrap().translation
        };
        assert_eq!(at(&mut app, free), Vec3::new(5.0, 0.0, 0.0));
        assert_eq!(at(&mut app, held), Vec3::ZERO, "a locked object moved");

        // Despawn is refused too — the delete key is the edit locking exists for.
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "delete".into(),
                gesture: None,
                ops: vec![Op::Despawn { id: held }],
            });
        });
        assert!(app.world().resource::<SceneIndex>().get(&held).is_some());

        // And unlocking releases it, in the same one place.
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "unlock".into(),
                gesture: None,
                ops: vec![Op::Remove {
                    target: held,
                    type_path: <crate::lock::Locked as bevy::reflect::TypePath>::type_path().into(),
                }],
            });
        });
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "move".into(),
                gesture: None,
                ops: vec![Op::Set {
                    target: held,
                    value: Box::new(moved).into_partial_reflect(),
                }],
            });
        });
        assert_eq!(at(&mut app, held), Vec3::new(5.0, 0.0, 0.0));
    }

    /// A refused transaction must not leave a phantom undo entry: pressing undo
    /// after "nothing happened" would then unwind the edit BEFORE it.
    #[test]
    fn a_wholly_refused_transaction_records_no_history() {
        let mut app = test_app();
        let held = SceneId::random();
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: held,
                    components: vec![
                        Box::new(crate::lock::Locked).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        });
        let depth = app.world().resource::<History>().undo_depth();
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "move".into(),
                gesture: None,
                ops: vec![Op::Set {
                    target: held,
                    value: Box::new(Transform::from_xyz(1.0, 2.0, 3.0)).into_partial_reflect(),
                }],
            });
        });
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth,
            "a refused edit put an empty step on the undo stack"
        );
    }
}
