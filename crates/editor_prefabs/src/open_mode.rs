//! In-place prefab opening (UX redesign, supersedes the world-swap edit mode):
//! `Enter` on an instance "opens" it like a folder — the scene NEVER leaves the
//! world. While open, editing is scoped to the instance's members
//! (`SelectionScope`), newly inserted entities auto-adopt under the root, and a
//! persistent chip + viewport border say exactly which reality you're in.
//! `Escape` (or Enter again) closes: the members' current structure IS the new
//! template — saved, propagated to every instance via the library generation.
//!
//! Because the world stays intact, the play/save/reload state-corruption class
//! of the old world-swap design structurally disappears; scene save and
//! play/reload are still gated while open (members mid-edit include not-yet-
//! stamped entities that would half-serialize).

use crate::{
    PrefabDef, PrefabInstance, PrefabLibrary, PrefabOverrides, StampedFrom, authoring,
    closure_contains,
};
use bevy::prelude::*;
use editor_core::edits::EditorComponents;
use editor_core::prelude::*;
use editor_scene::{PrefabStamped, SceneIoLock, snapshot_from_parts};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

pub struct OpenData {
    pub root: SceneId,
    pub prefab: Uuid,
    pub name: String,
    /// Top-level loose entities that existed at open time — bystanders the
    /// adopt-on-insert pass must NEVER pull into the group (explicit set, not
    /// `Added` change ticks: exclusive-system tick semantics are too slippery
    /// to bet scene structure on).
    pub leave_alone: HashSet<Entity>,
}

/// The instance currently open for structural editing. `None` = normal editing.
#[derive(Resource, Default)]
pub struct OpenInstance(pub Option<OpenData>);

/// Toggle: open the selected instance, or close the open one.
pub(crate) fn toggle_open(world: &mut World) {
    if world.resource::<OpenInstance>().0.is_some() {
        close(world);
    } else {
        open(world);
    }
}

/// Resolve the instance root from the selection (root itself or a stamped child).
fn selected_root(world: &mut World) -> Option<SceneId> {
    let mut query = world.query_filtered::<(
        Option<&PrefabInstance>,
        Option<&StampedFrom>,
        &SceneId,
    ), With<Selected>>();
    let mut roots: Vec<SceneId> = query
        .iter(world)
        .filter_map(|(instance, stamped, id)| {
            instance.map(|_| *id).or(stamped.map(|s| s.instance_root))
        })
        .collect();
    roots.sort_by_key(|id| id.0);
    roots.dedup();
    roots.first().copied()
}

fn open(world: &mut World) {
    let Some(root_id) = selected_root(world) else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select a prefab instance to open".into(),
            success: false,
        });
        return;
    };
    let Some(root_entity) = world.resource::<SceneIndex>().get(&root_id) else {
        return;
    };
    let Some(instance) = world.get::<PrefabInstance>(root_entity).copied() else {
        return;
    };
    let name = world
        .resource::<PrefabLibrary>()
        .prefabs
        .get(&instance.0)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "prefab".into());
    let leave_alone: HashSet<Entity> = loose_top_level(world);
    world.resource_mut::<SceneIoLock>().0 = true;
    world.resource_mut::<OpenInstance>().0 = Some(OpenData {
        root: root_id,
        prefab: instance.0,
        name: name.clone(),
        leave_alone,
    });
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("opened ◆ {name} — edit inside · ⎋ closes & saves"),
        success: true,
    });
}

/// Unparented scene entities that aren't stamped children or instance roots —
/// the pool the adopt pass draws from (minus the at-open bystanders).
fn loose_top_level(world: &mut World) -> HashSet<Entity> {
    let mut query =
        world.query_filtered::<Entity, (With<SceneId>, Without<ChildOf>, Without<PrefabStamped>)>();
    query.iter(world).collect()
}

/// Members of the open instance: the root plus every descendant with a SceneId.
pub(crate) fn members_of(world: &mut World, root_entity: Entity) -> Vec<Entity> {
    let mut members = vec![root_entity];
    let mut stack = vec![root_entity];
    while let Some(current) = stack.pop() {
        if let Some(children) = world.get::<Children>(current) {
            for child in children.iter() {
                if world.get::<SceneId>(child).is_some() {
                    members.push(child);
                    stack.push(child);
                }
            }
        }
    }
    members
}

/// Capture traversal for CLOSE: like `members_of` but never descends INTO a
/// nested instance — the nested ROOT record joins the template (a reference),
/// its stamped subtree does not (instances never expand, D4/D6).
fn capture_members(world: &mut World, root_entity: Entity) -> Vec<Entity> {
    let mut members = Vec::new();
    let mut stack = vec![root_entity];
    while let Some(current) = stack.pop() {
        if let Some(children) = world.get::<Children>(current) {
            for child in children.iter() {
                if world.get::<SceneId>(child).is_none() {
                    continue;
                }
                members.push(child);
                if world.get::<PrefabInstance>(child).is_none() {
                    stack.push(child);
                }
            }
        }
    }
    members
}

fn close(world: &mut World) {
    let Some(open) = world.resource_mut::<OpenInstance>().0.take() else {
        return;
    };
    world.resource_mut::<SceneIoLock>().0 = false;
    world
        .resource_mut::<editor_core::prelude::SelectionScope>()
        .0 = None;

    let Some(root_entity) = world.resource::<SceneIndex>().get(&open.root) else {
        return;
    };

    // The members' CURRENT structure is the new template (nested instance
    // roots join as REFERENCES; their stamped subtrees never do).
    let members: Vec<Entity> = capture_members(world, root_entity);

    // Full-chain cycle guard (D6): no member instance may reference — through
    // any nesting chain — the prefab being edited.
    let self_reference = members.iter().any(|e| {
        world.get::<PrefabInstance>(*e).is_some_and(|i| {
            i.0 == open.prefab
                || closure_contains(world.resource::<PrefabLibrary>(), i.0, open.prefab)
        })
    });
    if self_reference {
        world.resource_mut::<OpenInstance>().0 = Some(open);
        world.resource_mut::<SceneIoLock>().0 = true;
        world.write_message(editor_scene::SceneIoFeedback {
            message: "a prefab cannot contain itself — remove the nested instance".into(),
            success: false,
        });
        return;
    }

    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    let components = world.resource::<EditorComponents>().types.clone();

    // Template ids: keep the original template id for surviving stamped
    // entities (override patches stay valid); new entities keep their SceneId.
    let template_id_of: HashMap<Entity, SceneId> = members
        .iter()
        .map(|entity| {
            let id = world
                .get::<StampedFrom>(*entity)
                .map(|s| s.template_id)
                .or_else(|| world.get::<SceneId>(*entity).copied())
                .unwrap_or_default();
            (*entity, id)
        })
        .collect();
    let member_set: HashSet<Entity> = members.iter().copied().collect();

    let records: Vec<(
        SceneId,
        Option<SceneId>,
        Vec<Box<dyn bevy::reflect::PartialReflect>>,
    )> = members
        .iter()
        .map(|entity| {
            let values: Vec<Box<dyn bevy::reflect::PartialReflect>> = components
                .iter()
                .filter_map(|reg| {
                    let reflect_component = registry
                        .get(reg.type_id)?
                        .data::<bevy::ecs::reflect::ReflectComponent>()?;
                    let entity_ref = world.get_entity(*entity).ok()?;
                    Some(reflect_component.reflect(entity_ref)?.to_dynamic())
                })
                .collect();
            let parent = world
                .get::<ChildOf>(*entity)
                .map(|c| c.parent())
                .filter(|p| member_set.contains(p))
                .and_then(|p| template_id_of.get(&p).copied());
            (template_id_of[entity], parent, values)
        })
        .collect();
    drop(registry);

    // Update the library + asset, clear this instance's deltas (its state IS
    // the source now), and propagate: the generation bump restamps EVERY
    // instance — including this one, whose members are replaced by fresh
    // stamped entities (so nothing loose can leak into a scene save).
    {
        let mut library = world.resource_mut::<PrefabLibrary>();
        if let Some(def) = library.prefabs.get_mut(&open.prefab) {
            def.template = snapshot_from_parts(records);
        }
    }
    if let Some(mut overrides) = world.get_mut::<PrefabOverrides>(root_entity) {
        overrides.0.clear();
    }
    for entity in members {
        if let Ok(entity) = world.get_entity_mut(entity) {
            entity.despawn();
        }
    }
    let def_clone = clone_def(world, open.prefab);
    if let Some(def) = def_clone {
        authoring::save_prefab_public(world, &def);
    }
    world.resource_mut::<PrefabLibrary>().generation += 1;

    // Land selection on the instance root — you're holding what you just edited.
    if let Some(root_entity) = world.resource::<SceneIndex>().get(&open.root) {
        let previous: Vec<Entity> = {
            let mut query = world.query_filtered::<Entity, With<Selected>>();
            query.iter(world).collect()
        };
        for entity in previous {
            world.entity_mut(entity).remove::<Selected>();
        }
        world.entity_mut(root_entity).insert(Selected);
        world.write_message(SelectionChanged);
    }
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("◆ {} saved — all instances updated", open.name),
        success: true,
    });
}

pub(crate) fn clone_def(world: &World, prefab: Uuid) -> Option<PrefabDef> {
    let library = world.resource::<PrefabLibrary>();
    library.prefabs.get(&prefab).map(|p| PrefabDef {
        id: p.id,
        name: p.name.clone(),
        template: snapshot_from_parts(
            p.template
                .records()
                .map(|(id, parent, c)| (id, parent, c.iter().map(|v| v.to_dynamic()).collect()))
                .collect(),
        ),
    })
}

/// While open: keep the selection scope on the members, adopt newly inserted
/// entities under the root, and close on Escape (one layer per press — only
/// when the viewport owns focus in normal mode).
pub(crate) fn maintain_open_instance(world: &mut World) {
    let Some((root_id, leave_alone)) = world
        .resource::<OpenInstance>()
        .0
        .as_ref()
        .map(|o| (o.root, o.leave_alone.clone()))
    else {
        return;
    };
    let Some(root_entity) = world.resource::<SceneIndex>().get(&root_id) else {
        // Root vanished (undo of the grouping, deletion): drop out cleanly.
        world.resource_mut::<OpenInstance>().0 = None;
        world.resource_mut::<SceneIoLock>().0 = false;
        world
            .resource_mut::<editor_core::prelude::SelectionScope>()
            .0 = None;
        return;
    };

    // Adopt fresh top-level spawns (place_on_click, paste, nested prefab
    // placement) into the open group; bystanders from before the open stay
    // untouched; instances that would close a nesting cycle are refused loudly.
    let open_prefab = world
        .resource::<OpenInstance>()
        .0
        .as_ref()
        .map(|o| o.prefab);
    let fresh: Vec<Entity> = loose_top_level(world)
        .into_iter()
        .filter(|e| *e != root_entity && !leave_alone.contains(e))
        .collect();
    for entity in fresh {
        if let (Some(target), Some(instance)) =
            (open_prefab, world.get::<PrefabInstance>(entity).copied())
        {
            let cycles = instance.0 == target
                || closure_contains(world.resource::<PrefabLibrary>(), instance.0, target);
            if cycles {
                world.write_message(editor_scene::SceneIoFeedback {
                    message: "a prefab cannot contain itself — placed at scene root instead".into(),
                    success: false,
                });
                // Mark it a bystander so the refusal happens ONCE.
                if let Some(open) = world.resource_mut::<OpenInstance>().0.as_mut() {
                    open.leave_alone.insert(entity);
                }
                continue;
            }
        }
        world.entity_mut(entity).insert(ChildOf(root_entity));
    }

    let members: HashSet<Entity> = members_of(world, root_entity).into_iter().collect();
    world
        .resource_mut::<editor_core::prelude::SelectionScope>()
        .0 = Some(members);
}

/// Close requested via Escape (layering resolved at collect time) or toggle.
pub(crate) fn request_close(world: &mut World) {
    if world.resource::<OpenInstance>().0.is_some() {
        close(world);
    }
}

/// `prefab.flatten` (owner ask: imported assets carry undesirable hierarchies):
/// while an instance is OPEN, every member becomes a DIRECT child of the root,
/// world pose preserved, as ONE undoable transaction. Nested instances move as
/// units (their stamped subtrees are never ripped apart). Esc then saves the
/// flat structure as the template.
pub(crate) fn flatten_open(world: &mut World) {
    let Some(root_id) = world.resource::<OpenInstance>().0.as_ref().map(|o| o.root) else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "open a prefab instance to flatten it".into(),
            success: false,
        });
        return;
    };
    let Some(root_entity) = world.resource::<SceneIndex>().get(&root_id) else {
        return;
    };

    // Capture set = template members (stops inside nested instances).
    let members = capture_members(world, root_entity);
    let mut ops = Vec::new();
    for entity in &members {
        let direct = world
            .get::<ChildOf>(*entity)
            .is_some_and(|c| c.parent() == root_entity);
        if direct {
            continue;
        }
        let Some(id) = world.get::<SceneId>(*entity).copied() else {
            continue;
        };
        // Root-relative pose from the local-transform chain (GlobalTransform
        // may be stale mid-frame after this frame's edits).
        let mut acc = world.get::<Transform>(*entity).copied().unwrap_or_default();
        let mut current = *entity;
        while let Some(parent) = world.get::<ChildOf>(current).map(|c| c.parent()) {
            if parent == root_entity {
                break;
            }
            let parent_local = world.get::<Transform>(parent).copied().unwrap_or_default();
            acc = parent_local.mul_transform(acc);
            current = parent;
        }
        ops.push(Op::Reparent {
            target: id,
            parent: Some(root_id),
        });
        ops.push(Op::Set {
            target: id,
            value: Box::new(acc).into_partial_reflect(),
        });
    }
    if ops.is_empty() {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "already flat".into(),
            success: true,
        });
        return;
    }
    let moved = ops.len() / 2;
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: "Flatten Prefab".into(),
        gesture: None,
        ops,
    });
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("flattened — {moved} entities now direct children · ⎋ saves"),
        success: true,
    });
}
