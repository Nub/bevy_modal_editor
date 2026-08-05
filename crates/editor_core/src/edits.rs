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
    mut requests: ResMut<HistoryRequests>,
) {
    for invoked in reader.read() {
        // Global bindings, editor-gated semantics: undo/redo never fire while the
        // game owns input (play sessions must not eat the history).
        if !state.active {
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
            let entity = resolve(world, &id)?;
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

    for transaction in queue {
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
}
