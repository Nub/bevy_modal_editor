//! The kernel's edit engine (RFC §5, M2): drains `EditQueue` at `EditorSet::Mutate`,
//! applies ops via reflection, captures inverses generically, records history with
//! gesture coalescing under the FIRST-old-value contract (spike finding F1), and
//! services undo/redo. The only code in the editor that mutates scene components.

use bevy::ecs::lifecycle::{Add, Remove};
use bevy::ecs::relationship::RelationshipHookMode;
use bevy::prelude::*;
use bevy::reflect::{PartialReflect, TypeRegistry};
use editor_api::edits::Derived;
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
    /// Types the editor OWNS and must never save, whatever a level file says.
    ///
    /// `apply_scene` adopts any type it finds in a file, which is the door a
    /// source-level fitness test cannot watch: a hand-edited or foreign
    /// `level.ron` naming `Visibility` would pull it into the save set at load
    /// and start persisting hidden-ness into published levels. Hidden is a view
    /// on the level, never part of it (see `editor_core::hide`).
    pub const NEVER_SAVED: &'static [&'static str] = &[
        "bevy_camera::visibility::Visibility",
        "bevy_camera::visibility::InheritedVisibility",
        "bevy_camera::visibility::ViewVisibility",
    ];

    /// Adopt a type into the save allow-list at runtime (owner rule: anything
    /// a user INSERTS must persist; only system-derived state stays out).
    /// The type must already live in the `AppTypeRegistry`.
    pub fn adopt(&mut self, type_id: std::any::TypeId, type_path: &'static str) {
        if Self::NEVER_SAVED.contains(&type_path) {
            return;
        }
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

/// Apply one VALUE op — everything that touches a single entity's components or
/// its parentage. Returns its inverse, or `None` when the op degenerated to a
/// no-op. Despawn is deliberately NOT here: it is the one op whose inverse is a
/// whole subtree, and it lives in `apply_op`.
fn apply_value_op(
    world: &mut World,
    registry: &TypeRegistry,
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
            // Spawning onto a live id would orphan the original: `index_on_add`
            // overwrites the entry and nothing points at the old entity again.
            if world.resource::<SceneIndex>().get(&id).is_some() {
                warn!("Op::Spawn id {id:?} is already in the scene — skipped");
                return None;
            }
            let entity = world.spawn(id).id();
            for value in components {
                let Some((reflect_component, _)) = reflect_component_for(registry, value.as_ref())
                else {
                    continue;
                };
                // `continue`, never `?`: the entity is already spawned and
                // indexed above, so returning here would leave a live SceneId
                // with no inverse — exactly the ghost this slice exists to
                // stop, inside the op every restore is built from.
                let Ok(mut entity_mut) = world.get_entity_mut(entity) else {
                    continue;
                };
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
        // Handled by `apply_op`: its inverse is a whole subtree, not one op.
        Op::Despawn { .. } => None,
        Op::Reparent { target, parent } => {
            let entity = resolve(world, &target)?;
            let old_parent = world
                .get::<ChildOf>(entity)
                .and_then(|c| world.get::<SceneId>(c.parent()))
                .copied();
            match parent {
                Some(parent_id) => {
                    // A parent that no longer resolves would otherwise take the
                    // whole op down with it — no reparent, no inverse, and no
                    // word — which is how something lands somewhere nobody
                    // chose. Paste resolves its parent before building the op;
                    // this is the backstop for the next verb that does not.
                    let Some(parent_entity) = resolve(world, &parent_id) else {
                        warn!(
                            "Op::Reparent parent {parent_id:?} not in SceneIndex — \
                             {target:?} stays where it is"
                        );
                        return None;
                    };
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

/// One captured entity: its id, the id of the nearest RECORDED ancestor, and
/// its registered components.
type SubtreeRecord = (SceneId, Option<SceneId>, Vec<Box<dyn PartialReflect>>);

/// Everything a despawn is about to destroy, in an order that can rebuild it.
///
/// `world.entity_mut(e).despawn()` is RECURSIVE, so deleting a group deletes its
/// children too — but the inverse used to capture only the root, and undo handed
/// back a childless parent while the children were gone for good. It also
/// captures the ROOT's own parentage, because `Op::Spawn` always lands at the
/// world root: without it, undoing the deletion of a child returned that child
/// unparented.
///
/// `editor_components` is a PARAMETER and never read from the world: `apply_edits`
/// takes the resource out for the duration, so a `world.resource()` here would
/// capture nothing and undo would restore bare husks.
fn capture_subtree(
    world: &World,
    registry: &TypeRegistry,
    editor_components: &EditorComponents,
    root: Entity,
) -> Vec<SubtreeRecord> {
    // A DERIVED parent is never named: a stamp mints a fresh `SceneId` every
    // time it runs, so the reparent would dangle at the next restamp — and
    // adopting a restored entity into an instance would leak an expanded
    // instance into the level file.
    let root_parent = world
        .get::<ChildOf>(root)
        .map(|c| c.parent())
        .filter(|parent| world.get::<Derived>(*parent).is_none())
        .and_then(|parent| world.get::<SceneId>(parent).copied());

    let mut out: Vec<SubtreeRecord> = Vec::new();
    let mut stack: Vec<(Entity, Option<SceneId>)> = vec![(root, root_parent)];
    while let Some((entity, parent_id)) = stack.pop() {
        // Derived subtrees rebuild themselves, so capturing one would make undo
        // DUPLICATE it. The root is exempt: deleting a stamped member directly
        // behaves exactly as it did before, rather than gaining a new leak.
        if entity != root && world.get::<Derived>(entity).is_some() {
            continue;
        }
        let own = world.get::<SceneId>(entity).copied();
        if let Some(id) = own {
            let components = editor_components
                .types
                .iter()
                .filter_map(|reg| clone_component(world, registry, entity, reg.type_id))
                .collect();
            out.push((id, parent_id, components));
        }
        // Descend THROUGH entities the scene cannot name, carrying the nearest
        // recorded ancestor down: a scene child hanging under a plain bevy node
        // still belongs to the recorded grandparent.
        let child_link = own.or(parent_id);
        if let Some(children) = world.get::<Children>(entity) {
            let kids: Vec<Entity> = children.iter().collect();
            for child in kids.into_iter().rev() {
                stack.push((child, child_link));
            }
        }
    }
    out
}

/// Rebuild a captured subtree: every entity spawned before any of them is hung,
/// so a reparent always resolves.
fn restore_ops(records: Vec<SubtreeRecord>) -> Vec<Op> {
    let mut spawns = Vec::with_capacity(records.len());
    let mut reparents = Vec::new();
    for (id, parent, components) in records {
        spawns.push(Op::Spawn { id, components });
        if let Some(parent) = parent {
            // No op when the parent is None: `Op::Spawn` already lands at the root.
            reparents.push(Op::Reparent {
                target: id,
                parent: Some(parent),
            });
        }
    }
    spawns.extend(reparents);
    spawns
}

/// One entity in a COPY capture: its live id, the live id of the nearest
/// ancestor RECORDED IN THIS CAPTURE, and its registered components.
pub struct CopyRecord {
    pub id: SceneId,
    pub parent: Option<SceneId>,
    pub components: Vec<Box<dyn PartialReflect>>,
}

/// A whole subtree, ready to be stamped out any number of times.
pub struct CopySubtree {
    /// Always `records[0].id` — the walk records the root first.
    pub root: SceneId,
    /// What the ROOT hung under at capture, with derived parents filtered out.
    /// A hint: the caller decides whether to honour it.
    pub external_parent: Option<SceneId>,
    pub records: Vec<CopyRecord>,
    /// The root's pose in WORLD space at capture.
    ///
    /// A captured `Transform` is LOCAL, so a paste that cannot rejoin its
    /// parent would reinterpret it as a world pose and silently teleport — a
    /// child sitting 5 units inside a group at x=100 landing at x=5. This is
    /// what lets it keep its place instead.
    pub root_world: Transform,
}

impl CopySubtree {
    /// A deep copy of the values, because one capture is stamped N times and
    /// `PartialReflect` is not `Clone`.
    pub fn cloned(&self) -> CopySubtree {
        CopySubtree {
            root: self.root,
            root_world: self.root_world,
            external_parent: self.external_parent,
            records: self
                .records
                .iter()
                .map(|record| CopyRecord {
                    id: record.id,
                    parent: record.parent,
                    components: record.components.iter().map(|c| c.to_dynamic()).collect(),
                })
                .collect(),
        }
    }

    /// Rewrite the ROOT's `Transform` — an array's step, a paste's offset.
    ///
    /// Only the root: descendants are LOCAL to it and already ride along, so
    /// stepping them too would shear the copy apart.
    pub fn map_root_transform(&mut self, f: impl FnOnce(Transform) -> Transform) {
        let Some(record) = self.records.first_mut() else {
            return;
        };
        for value in &mut record.components {
            // The captured values are DYNAMIC, so a downcast never matches —
            // they have to be rebuilt through `FromReflect`. Getting this wrong
            // is silent: the step simply never applies and every copy lands on
            // its original.
            if value.get_represented_type_info().map(|info| info.type_id())
                != Some(std::any::TypeId::of::<Transform>())
            {
                continue;
            }
            let Some(transform) =
                <Transform as bevy::reflect::FromReflect>::from_reflect(value.as_ref())
            else {
                continue;
            };
            *value = Box::new(f(transform)).into_partial_reflect();
            return;
        }
    }
}

/// Why a root cannot be copied by value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CopyRefusal {
    /// The root is DERIVED: the editor regenerates it, so a copy is an orphan
    /// its producer will never own — and one that scene capture would
    /// serialize while restamping never cleans it up.
    DerivedRoot,
    /// Real scene content hangs BELOW a derived boundary. The walk cannot cross
    /// it — a stamp re-mints those ids every run — so a copy would silently
    /// drop whatever is under there. This is the one case the old blanket
    /// refusal was right about, and it is all that is left of it.
    LosesContentUnderDerived,
    /// The scene cannot name this entity.
    Unnamed,
}

/// A world pose composed from LOCAL transforms up the ancestor chain.
///
/// Deliberately not `GlobalTransform`: propagation runs once a frame, so an
/// entity spawned or reparented in THIS frame still carries a stale global —
/// and a capture taken then would record the wrong place. Composing the locals
/// is correct whenever it is asked, which also makes it testable in a kernel
/// that has no `TransformPlugin`.
fn world_pose(world: &World, entity: Entity) -> Transform {
    let mut affine = bevy::math::Affine3A::IDENTITY;
    let mut current = entity;
    loop {
        if let Some(local) = world.get::<Transform>(current) {
            affine = local.compute_affine() * affine;
        }
        match world.get::<ChildOf>(current) {
            Some(parent) => current = parent.parent(),
            None => break,
        }
    }
    Transform::from_matrix(affine.into())
}

fn has_named_content_below(world: &World, root: Entity) -> bool {
    let mut stack: Vec<Entity> = world
        .get::<Children>(root)
        .map(|kids| kids.iter().collect())
        .unwrap_or_default();
    while let Some(entity) = stack.pop() {
        if world.get::<SceneId>(entity).is_some() && world.get::<Derived>(entity).is_none() {
            return true;
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    false
}

/// Capture `root`'s whole subtree for COPYING.
///
/// This is deliberately NOT `capture_subtree`. That one serves undo, where ids
/// are preserved and a derived subtree is skipped because its producer will
/// rebuild it verbatim. A copy needs fresh ids, and skipping a derived subtree
/// would silently drop any real content hanging under it — so this walk probes
/// that case and REFUSES instead.
///
/// `editor_components` is a parameter and never read from the world, for the
/// same reason `capture_subtree` says so.
pub fn copy_subtree(
    world: &World,
    registry: &TypeRegistry,
    editor_components: &EditorComponents,
    root: Entity,
) -> Result<CopySubtree, CopyRefusal> {
    if world.get::<Derived>(root).is_some() {
        return Err(CopyRefusal::DerivedRoot);
    }
    let Some(root_id) = world.get::<SceneId>(root).copied() else {
        return Err(CopyRefusal::Unnamed);
    };
    // Same rule and reason as the undo capture: a derived parent is re-minted
    // every stamp, so naming one would dangle.
    let external_parent = world
        .get::<ChildOf>(root)
        .map(|c| c.parent())
        .filter(|parent| world.get::<Derived>(*parent).is_none())
        .and_then(|parent| world.get::<SceneId>(parent).copied());

    let mut records: Vec<CopyRecord> = Vec::new();
    let mut stack: Vec<(Entity, Option<SceneId>)> = vec![(root, None)];
    while let Some((entity, parent_id)) = stack.pop() {
        if entity != root && world.get::<Derived>(entity).is_some() {
            if has_named_content_below(world, entity) {
                return Err(CopyRefusal::LosesContentUnderDerived);
            }
            continue;
        }
        let own = world.get::<SceneId>(entity).copied();
        if let Some(id) = own {
            records.push(CopyRecord {
                id,
                parent: parent_id,
                components: editor_components
                    .types
                    .iter()
                    .filter_map(|reg| clone_component(world, registry, entity, reg.type_id))
                    .collect(),
            });
        }
        // Descend THROUGH entities the scene cannot name, carrying the nearest
        // recorded ancestor down.
        let child_link = own.or(parent_id);
        if let Some(children) = world.get::<Children>(entity) {
            let kids: Vec<Entity> = children.iter().collect();
            for child in kids.into_iter().rev() {
                stack.push((child, child_link));
            }
        }
    }
    Ok(CopySubtree {
        root_world: world_pose(world, root),
        root: root_id,
        external_parent,
        records,
    })
}

/// Stamp one captured subtree out under FRESH ids.
///
/// Internal parent links are remapped to the new ids; `external` is what the
/// copy's ROOT hangs under, or `None` to leave it where `Op::Spawn` puts it.
/// Every `Spawn` precedes every `Reparent`, so a reparent always resolves.
pub fn copy_ops(subtree: &CopySubtree, external: Option<SceneId>) -> (Vec<Op>, SceneId) {
    let fresh: Vec<SceneId> = subtree.records.iter().map(|_| SceneId::random()).collect();
    let remap = |old: SceneId| -> Option<SceneId> {
        subtree
            .records
            .iter()
            .position(|record| record.id == old)
            .map(|index| fresh[index])
    };
    let mut spawns = Vec::with_capacity(subtree.records.len());
    let mut reparents = Vec::new();
    // A root that had a parent and is not getting one back is about to become
    // a top-level object, so its captured LOCAL transform would be read as a
    // world pose and teleport it. Hand it the world pose it actually had.
    let orphaned = subtree.external_parent.is_some() && external.is_none();
    for (index, record) in subtree.records.iter().enumerate() {
        let mut components: Vec<Box<dyn PartialReflect>> =
            record.components.iter().map(|c| c.to_dynamic()).collect();
        if orphaned && index == 0 {
            for value in &mut components {
                if value.get_represented_type_info().map(|info| info.type_id())
                    == Some(std::any::TypeId::of::<Transform>())
                {
                    *value = Box::new(subtree.root_world).into_partial_reflect();
                    break;
                }
            }
        }
        spawns.push(Op::Spawn {
            id: fresh[index],
            components,
        });
        let parent = match record.parent {
            Some(old) => remap(old),
            None => external,
        };
        if let Some(parent) = parent {
            reparents.push(Op::Reparent {
                target: fresh[index],
                parent: Some(parent),
            });
        }
    }
    let root = fresh.first().copied().unwrap_or_else(SceneId::random);
    spawns.extend(reparents);
    (spawns, root)
}

/// Apply one op, returning its inverse as a LIST — empty for a no-op.
///
/// Only despawn produces more than one, and that is the whole point: its
/// inverse has to respawn a subtree and then re-hang it.
fn apply_op(
    world: &mut World,
    registry: &TypeRegistry,
    editor_components: &EditorComponents,
    op: Op,
    touched: &mut Vec<SceneId>,
    removed_here: &mut std::collections::HashSet<SceneId>,
) -> Vec<Op> {
    let Op::Despawn { id } = op else {
        return apply_value_op(world, registry, op, touched)
            .into_iter()
            .collect();
    };
    let Some(entity) = world.resource::<SceneIndex>().get(&id) else {
        // A despawn that silently no-ops leaves ghosts behind and is worth
        // hearing about — EXCEPT when an earlier op in this same transaction
        // already took it, which is what a children-first delete list does.
        if !removed_here.contains(&id) {
            warn!("Op::Despawn target {id:?} not in SceneIndex — skipped");
        }
        return Vec::new();
    };
    let records = capture_subtree(world, registry, editor_components, entity);
    for (captured, _, _) in &records {
        // `Edited` must name the WHOLE subtree: a listener that only heard about
        // the root would keep stale state for everything under it.
        touched.push(*captured);
        removed_here.insert(*captured);
    }
    world.entity_mut(entity).despawn();
    restore_ops(records)
}

fn apply_ops(
    world: &mut World,
    registry: &TypeRegistry,
    editor_components: &EditorComponents,
    ops: Vec<Op>,
    touched: &mut Vec<SceneId>,
) -> Vec<Op> {
    let mut removed_here: std::collections::HashSet<SceneId> = Default::default();
    let mut inverse: Vec<Vec<Op>> = Vec::with_capacity(ops.len());
    for op in ops {
        let inv = apply_op(
            world,
            registry,
            editor_components,
            op,
            touched,
            &mut removed_here,
        );
        if !inv.is_empty() {
            inverse.push(inv);
        }
    }
    // The OUTER list reverses; each op's own inverse keeps its internal order.
    // Flattening first and reversing once would turn [Spawn(g), Spawn(a),
    // Reparent(a→g)] into [Reparent(a→g), Spawn(a), Spawn(g)] — the reparent
    // resolves nothing, drops its own inverse, and undo hands back a detached
    // child. That is the very symptom this function exists to fix.
    inverse.reverse();
    inverse.into_iter().flatten().collect()
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
        let (refused, subtree_refusal) = {
            let index = world.resource::<SceneIndex>();
            let locked: std::collections::HashSet<SceneId> = index
                .iter()
                .filter(|(_, entity)| world.get::<crate::lock::Locked>(**entity).is_some())
                .map(|(id, _)| *id)
                .collect();
            if locked.is_empty() {
                (0, false)
            } else {
                // Every id whose SUBTREE contains a lock, so a recursive
                // despawn cannot quietly take a locked piece with it. Built
                // from the locked entities upwards, which is cheap because
                // locks are rare — walking down from every op would not be.
                let mut holds: std::collections::HashSet<SceneId> = locked.clone();
                for id in &locked {
                    let Some(entity) = world.resource::<SceneIndex>().get(id) else {
                        continue;
                    };
                    let mut current = entity;
                    while let Some(parent) = world.get::<ChildOf>(current).map(|c| c.parent()) {
                        if let Some(ancestor) = world.get::<SceneId>(parent) {
                            holds.insert(*ancestor);
                        }
                        current = parent;
                    }
                }
                let before = transaction.ops.len();
                let mut only_a_part = false;
                transaction.ops.retain(|op| {
                    let refused = crate::lock::op_is_refused(
                        op,
                        |id| locked.contains(&id),
                        |id| holds.contains(&id),
                    );
                    // Remember WHY, so the message can tell the difference
                    // between "that is locked" and "that contains something
                    // locked" — which are different problems with different
                    // fixes.
                    if refused
                        && let editor_api::edits::Op::Despawn { id } = op
                        && !locked.contains(id)
                    {
                        only_a_part = true;
                    }
                    !refused
                });
                (before - transaction.ops.len(), only_a_part)
            }
        };
        if refused > 0 && transaction.ops.is_empty() {
            // Say so, in the statusbar and not the log: a verb that silently
            // does nothing reads as a broken editor, and "it is locked" is the
            // whole answer. Every frame of a drag re-says it, which is right —
            // the message should stay lit for as long as you keep trying.
            world.write_message(editor_api::feedback::SceneIoFeedback {
                message: if subtree_refusal {
                    format!(
                        "{refused} object{} hold{} a locked part \u{00b7} \u{2423}l to unlock it",
                        if refused == 1 { "" } else { "s" },
                        if refused == 1 { "s" } else { "" }
                    )
                } else {
                    format!(
                        "{refused} locked object{} \u{00b7} \u{2423}l to unlock",
                        if refused == 1 { "" } else { "s" }
                    )
                },
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

    /// `apply_scene` adopts any type a level file names, which is the door the
    /// source-level fitness test cannot watch: a hand-edited or foreign
    /// `level.ron` carrying a `Visibility` record would pull the type into the
    /// save set at load and start persisting hidden-ness into published levels.
    #[test]
    fn the_save_set_refuses_editor_owned_visibility() {
        let mut components = EditorComponents::default();
        for type_path in EditorComponents::NEVER_SAVED {
            components.adopt(std::any::TypeId::of::<Visibility>(), type_path);
        }
        assert!(
            components.types.is_empty(),
            "a level file talked the editor into saving visibility"
        );
        // Anything else still adopts — this is a deny-list, not a freeze.
        components.adopt(
            std::any::TypeId::of::<Health>(),
            <Health as bevy::reflect::TypePath>::type_path(),
        );
        assert_eq!(components.types.len(), 1);
    }

    fn spawn_tree(app: &mut App, ids: &[(SceneId, Option<SceneId>, f32)]) {
        let mut ops = Vec::new();
        for (id, _, health) in ids {
            ops.push(Op::Spawn {
                id: *id,
                components: vec![
                    Box::new(Health {
                        current: *health,
                        max: 10.0,
                    })
                    .into_partial_reflect(),
                    Box::new(Transform::default()).into_partial_reflect(),
                ],
            });
        }
        for (id, parent, _) in ids {
            if let Some(parent) = parent {
                ops.push(Op::Reparent {
                    target: *id,
                    parent: Some(*parent),
                });
            }
        }
        edit(app, |q| {
            q.0.push(Transaction {
                label: "spawn tree".into(),
                gesture: None,
                ops,
            });
        });
    }

    fn entity_of(app: &mut App, id: SceneId) -> Option<Entity> {
        app.world().resource::<SceneIndex>().get(&id)
    }

    fn parent_of(app: &mut App, id: SceneId) -> Option<SceneId> {
        let entity = entity_of(app, id)?;
        let world = app.world_mut();
        let parent = world.get::<ChildOf>(entity)?.parent();
        world.get::<SceneId>(parent).copied()
    }

    /// THE bug. `despawn()` is recursive but the inverse captured only the root,
    /// so deleting a group and pressing undo handed back a childless parent
    /// while the children were gone for good — silent, permanent data loss in
    /// the two most-used verbs in any editor.
    #[test]
    fn undoing_a_delete_restores_the_whole_subtree() {
        let mut app = test_app();
        let (root, mid, leaf) = (SceneId::random(), SceneId::random(), SceneId::random());
        spawn_tree(
            &mut app,
            &[
                (root, None, 1.0),
                (mid, Some(root), 2.0),
                (leaf, Some(mid), 3.0),
            ],
        );

        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "delete".into(),
                gesture: None,
                ops: vec![Op::Despawn { id: root }],
            });
        });
        for id in [root, mid, leaf] {
            assert!(
                entity_of(&mut app, id).is_none(),
                "{id:?} survived a delete"
            );
        }

        undo(&mut app, 1);
        for (id, health) in [(root, 1.0), (mid, 2.0), (leaf, 3.0)] {
            let entity =
                entity_of(&mut app, id).unwrap_or_else(|| panic!("{id:?} did not come back"));
            assert_eq!(
                app.world_mut().get::<Health>(entity).unwrap().current,
                health,
                "{id:?} came back with the wrong values"
            );
        }
        // And the SHAPE comes back, not just the entities.
        assert_eq!(parent_of(&mut app, mid), Some(root));
        assert_eq!(parent_of(&mut app, leaf), Some(mid));
    }

    /// The second half of the same bug: `Op::Spawn` always lands at the world
    /// root, so undoing the deletion of a CHILD used to return it unparented —
    /// the object was back, in the wrong place in the tree, with nothing said.
    #[test]
    fn undoing_a_deleted_child_puts_it_back_under_its_parent() {
        let mut app = test_app();
        let (root, child) = (SceneId::random(), SceneId::random());
        spawn_tree(&mut app, &[(root, None, 1.0), (child, Some(root), 2.0)]);

        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "delete child".into(),
                gesture: None,
                ops: vec![Op::Despawn { id: child }],
            });
        });
        assert!(entity_of(&mut app, child).is_none());
        assert!(entity_of(&mut app, root).is_some(), "the parent went too");

        undo(&mut app, 1);
        assert_eq!(
            parent_of(&mut app, child),
            Some(root),
            "the child came back at the world root"
        );
    }

    /// Redo has to destroy exactly what undo restored, or the second undo
    /// resurrects a subtree that should have stayed deleted.
    #[test]
    fn delete_undo_redo_round_trips() {
        let mut app = test_app();
        let (root, child) = (SceneId::random(), SceneId::random());
        spawn_tree(&mut app, &[(root, None, 1.0), (child, Some(root), 2.0)]);
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "delete".into(),
                gesture: None,
                ops: vec![Op::Despawn { id: root }],
            });
        });
        undo(&mut app, 1);
        redo(&mut app, 1);
        for id in [root, child] {
            assert!(
                entity_of(&mut app, id).is_none(),
                "{id:?} survived the redo"
            );
        }
        undo(&mut app, 1);
        assert_eq!(
            parent_of(&mut app, child),
            Some(root),
            "the second undo lost the shape"
        );
    }

    /// A transaction that deletes a parent AND one of its children — which a cut
    /// of a multi-selection produces — must not warn, must not double-restore,
    /// and must come back once.
    #[test]
    fn deleting_a_parent_and_its_child_in_one_transaction_restores_once() {
        let mut app = test_app();
        let (root, child) = (SceneId::random(), SceneId::random());
        spawn_tree(&mut app, &[(root, None, 1.0), (child, Some(root), 2.0)]);
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "delete both".into(),
                gesture: None,
                ops: vec![Op::Despawn { id: root }, Op::Despawn { id: child }],
            });
        });
        undo(&mut app, 1);
        let count = app
            .world_mut()
            .query_filtered::<&SceneId, ()>()
            .iter(app.world())
            .filter(|id| **id == child)
            .count();
        assert_eq!(count, 1, "the child came back {count} times");
        assert_eq!(parent_of(&mut app, child), Some(root));
    }

    /// DERIVED children rebuild themselves, so capturing them would make undo
    /// duplicate the subtree — and naming one as a parent would dangle, because
    /// a stamp mints fresh ids every time it runs.
    #[test]
    fn a_derived_child_is_not_captured() {
        let mut app = test_app();
        let (root, derived) = (SceneId::random(), SceneId::random());
        spawn_tree(&mut app, &[(root, None, 1.0), (derived, Some(root), 2.0)]);
        let entity = entity_of(&mut app, derived).unwrap();
        app.world_mut()
            .entity_mut(entity)
            .insert(editor_api::edits::Derived);

        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "delete".into(),
                gesture: None,
                ops: vec![Op::Despawn { id: root }],
            });
        });
        undo(&mut app, 1);
        assert!(
            entity_of(&mut app, root).is_some(),
            "the root did not come back"
        );
        assert!(
            entity_of(&mut app, derived).is_none(),
            "undo restored a derived child its producer will rebuild — now there are two"
        );
    }

    fn copy_of(app: &mut App, root: SceneId) -> Result<CopySubtree, CopyRefusal> {
        let entity = app.world().resource::<SceneIndex>().get(&root).unwrap();
        let registry_arc = app.world().resource::<AppTypeRegistry>().clone();
        let owned = EditorComponents {
            types: app.world().resource::<EditorComponents>().types.clone(),
        };
        let registry = registry_arc.read();
        copy_subtree(app.world(), &registry, &owned, entity)
    }

    /// Every copy is a fresh id, and the internal links point at the COPIES.
    /// A childless-root regression passes a count test and fails this.
    #[test]
    fn copy_ops_hangs_the_copy_under_its_own_root() {
        let mut app = test_app();
        let (a, b, c) = (SceneId::random(), SceneId::random(), SceneId::random());
        spawn_tree(
            &mut app,
            &[(a, None, 1.0), (b, Some(a), 2.0), (c, Some(b), 3.0)],
        );
        let subtree = copy_of(&mut app, a).expect("a plain tree is copyable");
        let (ops, root) = copy_ops(&subtree, None);

        let spawned: Vec<SceneId> = ops
            .iter()
            .filter_map(|op| match op {
                Op::Spawn { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        assert_eq!(spawned.len(), 3);
        assert!(
            spawned.iter().all(|id| ![a, b, c].contains(id)),
            "a copy reused a live id"
        );
        // Every spawn precedes every reparent, or a reparent resolves nothing.
        let first_reparent = ops
            .iter()
            .position(|op| matches!(op, Op::Reparent { .. }))
            .unwrap();
        assert!(
            ops[..first_reparent]
                .iter()
                .all(|op| matches!(op, Op::Spawn { .. })),
            "a reparent came before a spawn"
        );
        let reparents: Vec<(SceneId, Option<SceneId>)> = ops
            .iter()
            .filter_map(|op| match op {
                Op::Reparent { target, parent } => Some((*target, *parent)),
                _ => None,
            })
            .collect();
        assert_eq!(reparents.len(), 2);
        assert!(
            reparents
                .iter()
                .all(|(_, parent)| parent.is_some_and(|p| spawned.contains(&p))),
            "a copy was hung under an ORIGINAL instead of its own copy"
        );
        assert_eq!(root, spawned[0]);
    }

    /// The root is recorded first and is the ONLY record with no in-capture
    /// parent — which is what makes the external hook root-only.
    #[test]
    fn a_copy_capture_records_its_root_first() {
        let mut app = test_app();
        let (a, b) = (SceneId::random(), SceneId::random());
        spawn_tree(&mut app, &[(a, None, 1.0), (b, Some(a), 2.0)]);
        let subtree = copy_of(&mut app, a).unwrap();
        assert_eq!(subtree.records[0].id, a);
        assert_eq!(
            subtree
                .records
                .iter()
                .filter(|r| r.parent.is_none())
                .count(),
            1
        );
    }

    /// The walk descends THROUGH entities the scene cannot name, carrying the
    /// nearest recorded ancestor down — easy to lose when ids are re-minted.
    #[test]
    fn the_copy_walk_descends_through_an_unnamed_entity() {
        let mut app = test_app();
        let (a, c) = (SceneId::random(), SceneId::random());
        spawn_tree(&mut app, &[(a, None, 1.0), (c, None, 3.0)]);
        let a_entity = app.world().resource::<SceneIndex>().get(&a).unwrap();
        let c_entity = app.world().resource::<SceneIndex>().get(&c).unwrap();
        let bridge = app.world_mut().spawn(ChildOf(a_entity)).id();
        app.world_mut().entity_mut(c_entity).insert(ChildOf(bridge));

        let subtree = copy_of(&mut app, a).unwrap();
        let record = subtree.records.iter().find(|r| r.id == c).unwrap();
        assert_eq!(
            record.parent,
            Some(a),
            "the unnamed bridge broke the parent link"
        );
    }

    /// A generated root is refused: the editor rebuilds it, so a copy is an
    /// orphan its producer will never own.
    #[test]
    fn copying_a_derived_root_is_refused() {
        let mut app = test_app();
        let a = SceneId::random();
        spawn_tree(&mut app, &[(a, None, 1.0)]);
        let entity = app.world().resource::<SceneIndex>().get(&a).unwrap();
        app.world_mut().entity_mut(entity).insert(Derived);
        assert_eq!(copy_of(&mut app, a).err(), Some(CopyRefusal::DerivedRoot));
    }

    /// THE narrow gate. Real content under a generated member cannot be reached
    /// by the walk, so a copy would silently drop it — refuse instead.
    #[test]
    fn a_copy_refuses_when_real_content_hides_under_a_derived_member() {
        let mut app = test_app();
        let (a, m, g) = (SceneId::random(), SceneId::random(), SceneId::random());
        spawn_tree(
            &mut app,
            &[(a, None, 1.0), (m, Some(a), 2.0), (g, Some(m), 3.0)],
        );
        let member = app.world().resource::<SceneIndex>().get(&m).unwrap();
        app.world_mut().entity_mut(member).insert(Derived);
        assert_eq!(
            copy_of(&mut app, a).err(),
            Some(CopyRefusal::LosesContentUnderDerived)
        );
    }

    /// A generated member with nothing real under it is simply skipped — the
    /// copy's own producer will rebuild it.
    #[test]
    fn a_derived_member_with_nothing_under_it_is_copied_around() {
        let mut app = test_app();
        let (a, m) = (SceneId::random(), SceneId::random());
        spawn_tree(&mut app, &[(a, None, 1.0), (m, Some(a), 2.0)]);
        let member = app.world().resource::<SceneIndex>().get(&m).unwrap();
        app.world_mut().entity_mut(member).insert(Derived);
        let subtree = copy_of(&mut app, a).unwrap();
        assert_eq!(subtree.records.len(), 1, "the generated member was copied");
    }

    /// Two walkers, one shape: the copy walk and the undo capture must agree on
    /// a plain tree, or they will drift apart the first time one is changed.
    #[test]
    fn copy_and_restore_agree_on_a_derived_free_tree() {
        let mut app = test_app();
        let (a, b, c) = (SceneId::random(), SceneId::random(), SceneId::random());
        spawn_tree(
            &mut app,
            &[(a, None, 1.0), (b, Some(a), 2.0), (c, Some(b), 3.0)],
        );
        let entity = app.world().resource::<SceneIndex>().get(&a).unwrap();
        let registry_arc = app.world().resource::<AppTypeRegistry>().clone();
        let owned = EditorComponents {
            types: app.world().resource::<EditorComponents>().types.clone(),
        };
        let registry = registry_arc.read();
        let undo = capture_subtree(app.world(), &registry, &owned, entity);
        let copy = copy_subtree(app.world(), &registry, &owned, entity).unwrap();
        assert_eq!(undo.len(), copy.records.len());
        for (record, (id, parent, _)) in copy.records.iter().zip(undo.iter()) {
            assert_eq!(record.id, *id);
            assert_eq!(record.parent, *parent);
        }
    }

    /// A lock has to hold against the delete that does not name it.
    ///
    /// `despawn()` is recursive, and `op_is_refused` matched the target id
    /// only — so locking a crate protected it from `d`, and did nothing at all
    /// if someone deleted the group it sat in. The lock promises to refuse
    /// EVERY edit until it is lifted; "unless you delete its parent" was not
    /// part of the promise.
    #[test]
    fn a_locked_child_refuses_its_parents_delete() {
        let mut app = test_app();
        let (parent, child) = (SceneId::random(), SceneId::random());
        spawn_tree(&mut app, &[(parent, None, 1.0), (child, Some(parent), 2.0)]);
        let entity = app.world().resource::<SceneIndex>().get(&child).unwrap();
        app.world_mut()
            .entity_mut(entity)
            .insert(crate::lock::Locked);

        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "delete the group".into(),
                gesture: None,
                ops: vec![Op::Despawn { id: parent }],
            });
        });
        assert!(
            app.world().resource::<SceneIndex>().get(&child).is_some(),
            "a locked child was destroyed by its parent's delete"
        );
        assert!(
            app.world().resource::<SceneIndex>().get(&parent).is_some(),
            "the parent went, orphaning the locked child it was refused for"
        );
    }

    /// The refusal names the RIGHT problem: "that is locked" and "that contains
    /// something locked" have different fixes, and a message that says the
    /// first when it means the second sends you looking at the wrong object.
    #[test]
    fn holding_a_locked_part_says_which_problem_it_is() {
        let mut app = test_app();
        let (parent, child) = (SceneId::random(), SceneId::random());
        spawn_tree(&mut app, &[(parent, None, 1.0), (child, Some(parent), 2.0)]);
        let entity = app.world().resource::<SceneIndex>().get(&child).unwrap();
        app.world_mut()
            .entity_mut(entity)
            .insert(crate::lock::Locked);
        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "delete the group".into(),
                gesture: None,
                ops: vec![Op::Despawn { id: parent }],
            });
        });

        let messages = app
            .world()
            .resource::<bevy::ecs::message::Messages<editor_api::feedback::SceneIoFeedback>>();
        let mut cursor = messages.get_cursor();
        let said: Vec<String> = cursor.read(messages).map(|m| m.message.clone()).collect();
        assert!(
            said.iter().any(|m| m.contains("locked part")),
            "the refusal blamed the wrong object: {said:?}"
        );
    }

    /// The rule is DESPAWN-only. Every other op edits exactly what it names, so
    /// a locked child rides its parent's move the way any child does — riding
    /// is not an edit, and refusing it would freeze the parent too.
    #[test]
    fn a_locked_child_does_not_freeze_its_parents_move() {
        let mut app = test_app();
        let (parent, child) = (SceneId::random(), SceneId::random());
        spawn_tree(&mut app, &[(parent, None, 1.0), (child, Some(parent), 2.0)]);
        let entity = app.world().resource::<SceneIndex>().get(&child).unwrap();
        app.world_mut()
            .entity_mut(entity)
            .insert(crate::lock::Locked);

        edit(&mut app, |q| {
            q.0.push(Transaction {
                label: "move the group".into(),
                gesture: None,
                ops: vec![Op::Set {
                    target: parent,
                    value: Box::new(Transform::from_xyz(5.0, 0.0, 0.0)).into_partial_reflect(),
                }],
            });
        });
        let moved = app.world().resource::<SceneIndex>().get(&parent).unwrap();
        assert_eq!(
            app.world_mut().get::<Transform>(moved).unwrap().translation,
            Vec3::new(5.0, 0.0, 0.0),
            "a locked child froze the parent it merely hangs under"
        );
    }
}
