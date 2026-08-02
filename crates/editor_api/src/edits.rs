//! The only door for scene mutation (RFC §5): features queue transactions of ops
//! against `SceneId` targets; the kernel applies them at `EditorSet::Mutate`,
//! captures inverses generically via reflection (M0 spike 1: 24–440× headroom), and
//! records history. Gesture coalescing keeps FIRST-old-value inverses (spike finding
//! F1): coalescing accumulates forward ops only and never touches the original
//! inverse, so undo of a whole gesture restores exactly.

use crate::ids::SceneId;
use bevy::prelude::*;
use bevy::reflect::PartialReflect;
use std::collections::HashMap;

/// Forward/inverse operation. Values are dynamic reflects carrying their represented
/// type (resolved against the `AppTypeRegistry` at apply time).
pub enum Op {
    /// Set (or insert) a component from a reflected value. Inverse: previous value,
    /// or `Remove` if the component was absent.
    Set { target: SceneId, value: Box<dyn PartialReflect> },
    /// Remove a component by stable type path. Inverse: `Set` of the captured value.
    Remove { target: SceneId, type_path: String },
    /// Spawn an entity with `SceneId` + reflected components. Inverse: `Despawn`.
    Spawn { id: SceneId, components: Vec<Box<dyn PartialReflect>> },
    /// Despawn; inverse re-spawns with all registered editor components captured.
    Despawn { id: SceneId },
    /// Reparent (None = root). Inverse: previous parent.
    Reparent { target: SceneId, parent: Option<SceneId> },
}

/// One atomic, labeled unit of history.
pub struct Transaction {
    pub label: String,
    /// Same gesture id across transactions ⇒ they coalesce into one history entry.
    pub gesture: Option<u64>,
    pub ops: Vec<Op>,
}

/// Queue drained by the kernel each frame at `EditorSet::Mutate`.
#[derive(Resource, Default)]
pub struct EditQueue(pub Vec<Transaction>);

/// The `SceneId → Entity` index, maintained by the kernel via `SceneId` lifecycle
/// observers. Ops resolve targets through this — never through cached `Entity` values.
#[derive(Resource, Default)]
pub struct SceneIndex {
    forward: HashMap<SceneId, Entity>,
}

impl SceneIndex {
    pub fn get(&self, id: &SceneId) -> Option<Entity> {
        self.forward.get(id).copied()
    }
    pub fn insert(&mut self, id: SceneId, entity: Entity) {
        self.forward.insert(id, entity);
    }
    pub fn remove(&mut self, id: &SceneId) {
        self.forward.remove(id);
    }
    pub fn len(&self) -> usize {
        self.forward.len()
    }
    pub fn is_empty(&self) -> bool {
        self.forward.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&SceneId, &Entity)> {
        self.forward.iter()
    }
}

/// The feature-facing mutation façade (RFC §5). Features never touch scene
/// components directly — the architectural fitness greps enforce it.
#[derive(bevy::ecs::system::SystemParam)]
pub struct EditScope<'w> {
    queue: ResMut<'w, EditQueue>,
}

impl EditScope<'_> {
    pub fn transaction(&mut self, label: impl Into<String>) -> TransactionBuilder<'_> {
        TransactionBuilder {
            queue: &mut self.queue,
            transaction: Transaction { label: label.into(), gesture: None, ops: Vec::new() },
        }
    }
}

pub struct TransactionBuilder<'a> {
    queue: &'a mut EditQueue,
    transaction: Transaction,
}

impl TransactionBuilder<'_> {
    /// Mark as part of a continuous gesture (drag frames, held-key repeats).
    pub fn gesture(mut self, id: u64) -> Self {
        self.transaction.gesture = Some(id);
        self
    }
    pub fn set(mut self, target: SceneId, value: impl PartialReflect) -> Self {
        self.transaction.ops.push(Op::Set { target, value: Box::new(value) });
        self
    }
    pub fn set_dynamic(mut self, target: SceneId, value: Box<dyn PartialReflect>) -> Self {
        self.transaction.ops.push(Op::Set { target, value });
        self
    }
    pub fn remove(mut self, target: SceneId, type_path: impl Into<String>) -> Self {
        self.transaction.ops.push(Op::Remove { target, type_path: type_path.into() });
        self
    }
    pub fn spawn(mut self, id: SceneId, components: Vec<Box<dyn PartialReflect>>) -> Self {
        self.transaction.ops.push(Op::Spawn { id, components });
        self
    }
    pub fn despawn(mut self, id: SceneId) -> Self {
        self.transaction.ops.push(Op::Despawn { id });
        self
    }
    pub fn reparent(mut self, target: SceneId, parent: Option<SceneId>) -> Self {
        self.transaction.ops.push(Op::Reparent { target, parent });
        self
    }
    /// Queue for application at `EditorSet::Mutate`. Dropping without commit aborts.
    pub fn commit(self) {
        if !self.transaction.ops.is_empty() {
            self.queue.0.push(self.transaction);
        }
    }
}

/// Broadcast after the kernel applies edits; derived systems (regenerate, gizmos,
/// save-dirty tracking) react to this, never to raw component change detection alone.
#[derive(Message, Debug)]
pub struct Edited {
    pub targets: Vec<SceneId>,
}
