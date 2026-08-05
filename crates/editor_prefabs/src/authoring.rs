//! Prefab authoring verbs (M4-D5 close): create-from-selection, revert
//! overrides, apply-to-prefab, library dir loading, generation-driven restamp.

use crate::{
    OverridePatch, PrefabDef, PrefabInstance, PrefabLibrary, PrefabOverrides, StampedFrom,
    stamp_prefab,
};
use bevy::prelude::*;
use editor_core::edits::EditorComponents;
use editor_core::prelude::*;
use editor_scene::{PrefabStamped, snapshot_from_parts};
use std::path::PathBuf;
use uuid::Uuid;

pub const PREFABS_DIR: &str = "prefabs";

#[derive(Resource, Default)]
pub(crate) struct PrefabRequests {
    revert: bool,
    apply: bool,
    open_toggle: bool,
    escape_close: bool,
    flatten: bool,
    /// Instance root to socket-snap after a move gesture commits (D10).
    snap_after_move: Option<SceneId>,
    repeat: bool,
}

/// What the inline name prompt is naming (title + commit routing).
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum PromptPurpose {
    #[default]
    Group,
    Variant,
}

/// The inline name prompt state (UI renders it; Enter commits a name here).
#[derive(Resource, Default)]
pub struct GroupPrompt {
    pub open: bool,
    pub purpose: PromptPurpose,
}

/// Set by the prompt UI on Enter; consumed by the group performer.
#[derive(Resource, Default)]
pub struct GroupCommit(pub Option<String>);

/// New instance to select once its spawn transaction has applied.
#[derive(Resource, Default)]
pub struct PendingGroupSelect(pub Option<SceneId>);

/// Runs BEFORE the resolver conventions (registered with .before), so the
/// mode/panel state observed here is the PRE-press state — Escape closes the
/// open instance only when nothing shallower (panel focus, non-normal mode,
/// capture) consumes that press. One layer per press, deterministically.
#[allow(clippy::too_many_arguments)]
pub(crate) fn collect_prefab_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<EditorState>,
    selection: Query<(), With<Selected>>,
    mode: Res<CurrentMode>,
    panel_focus: Res<PanelFocus>,
    escape_from_capture: Res<editor_core::resolver::EscapeFromCapture>,
    open: Res<crate::open_mode::OpenInstance>,
    mut prompt: ResMut<GroupPrompt>,
    mut requests: ResMut<PrefabRequests>,
    mut bake_requests: ResMut<crate::bake::BakeRequests>,
    gesture: Res<MoveGesture>,
    index: Res<SceneIndex>,
    instances: Query<(), With<PrefabInstance>>,
) {
    if !state.active {
        return;
    }
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "prefab.group" => {
                if !selection.is_empty() {
                    prompt.open = true;
                    prompt.purpose = PromptPurpose::Group;
                }
            }
            "prefab.make-variant" => {
                if !selection.is_empty() {
                    prompt.open = true;
                    prompt.purpose = PromptPurpose::Variant;
                }
            }
            "prefab.revert-overrides" => requests.revert = true,
            "prefab.apply-to-prefab" => requests.apply = true,
            "prefab.open" => requests.open_toggle = true,
            "prefab.flatten" => requests.flatten = true,
            "prefab.bake" => bake_requests.bake = true,
            "prefab.repeat" => requests.repeat = true,
            // D10: when a move gesture commits on a single prefab instance,
            // try to mate it with a nearby compatible socket. Collect runs
            // pre-conventions, so the gesture is still Active here.
            "transform.commit" => {
                if let MoveGesture::Active { originals, .. } = &*gesture
                    && let [(root_id, _)] = originals.as_slice()
                    && instances
                        .get(index.get(root_id).unwrap_or(Entity::PLACEHOLDER))
                        .is_ok()
                {
                    requests.snap_after_move = Some(*root_id);
                }
            }
            // One layer per press: a live SELECTION absorbs this Escape (the
            // selection handler clears it); only an empty-handed Escape closes.
            "core.escape-home"
                if open.0.is_some()
                    && !escape_from_capture.0
                    && panel_focus.0.is_none()
                    && mode.0 == editor_core::MODE_NORMAL
                    && selection.is_empty() =>
            {
                requests.escape_close = true;
            }
            _ => {}
        }
    }
}

/// Startup: load every prefabs/*.prefab.ron into the library.
pub(crate) fn load_prefab_library(world: &mut World) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let Ok(entries) = std::fs::read_dir(PREFABS_DIR) else {
        return;
    };
    let mut loaded = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.to_string_lossy().ends_with(".prefab.ron") {
            continue;
        }
        match PrefabDef::load(&path, &registry) {
            Ok(mut def) => {
                // Migrate legacy templates (pre-rebase flow) to the pivot
                // convention: top-level records centered on the root. Without
                // this, placing an old prefab stamps its parts meters away
                // from where the user pointed.
                if let Some(centered) = center_template(&def.template) {
                    def.template = centered;
                    if let Err(e) = def.save(&path, &registry) {
                        error!("prefab migration save failed for {}: {e}", path.display());
                    } else {
                        info!("prefab '{}' migrated to centered template", def.name);
                    }
                }
                world
                    .resource_mut::<PrefabLibrary>()
                    .prefabs
                    .insert(def.id, def);
                loaded += 1;
            }
            Err(e) => error!("prefab load failed for {}: {e}", path.display()),
        }
    }
    if loaded > 0 {
        info!("loaded {loaded} prefab(s)");
        world.resource_mut::<PrefabLibrary>().generation += 1;
    }
}

/// Concrete `Transform` from a snapshot value (which may be a DYNAMIC struct
/// fresh off the RON deserializer — `try_downcast_ref` would always miss).
fn reflect_transform(
    value: &(impl AsRef<dyn bevy::reflect::PartialReflect> + ?Sized),
) -> Option<Transform> {
    let value = value.as_ref();
    let is_transform = value
        .get_represented_type_info()
        .is_some_and(|i| i.type_path() == <Transform as bevy::reflect::TypePath>::type_path());
    if !is_transform {
        return None;
    }
    <Transform as bevy::reflect::FromReflect>::from_reflect(value)
}

/// `Some(centered)` if the template's top-level records aren't centered on the
/// root (centroid off origin beyond float noise) — the legacy-format migration.
pub(crate) fn center_template(
    template: &editor_scene::SceneSnapshot,
) -> Option<editor_scene::SceneSnapshot> {
    let mut centroid = Vec3::ZERO;
    let mut top_level = 0usize;
    for (_, parent, components) in template.records() {
        if parent.is_some() {
            continue;
        }
        if let Some(transform) = components.iter().find_map(reflect_transform) {
            centroid += transform.translation;
            top_level += 1;
        }
    }
    if top_level == 0 {
        return None;
    }
    centroid /= top_level as f32;
    centroid.y = 0.0; // members keep their heights (same rule as grouping)
    if centroid.length() < 1e-3 {
        return None;
    }
    let records = template
        .records()
        .map(|(id, parent, components)| {
            let values: Vec<Box<dyn bevy::reflect::PartialReflect>> = components
                .iter()
                .map(|c| {
                    if parent.is_none()
                        && let Some(mut rebased) = reflect_transform(c)
                    {
                        rebased.translation -= centroid;
                        return Box::new(rebased) as Box<dyn bevy::reflect::PartialReflect>;
                    }
                    c.to_dynamic()
                })
                .collect();
            (id, parent, values)
        })
        .collect();
    Some(editor_scene::snapshot_from_parts(records))
}

pub fn save_prefab_public(world: &World, def: &PrefabDef) {
    save_prefab(world, def)
}

fn save_prefab(world: &World, def: &PrefabDef) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let _ = std::fs::create_dir_all(PREFABS_DIR);
    let path = PathBuf::from(PREFABS_DIR).join(format!(
        "{}.prefab.ron",
        def.name.to_lowercase().replace(' ', "-")
    ));
    if let Err(e) = def.save(&path, &registry.read()) {
        error!("prefab save failed: {e}");
    }
}

/// The verbs (exclusive; scene mutations go through the EditQueue).
pub(crate) fn perform_prefab_actions(world: &mut World) {
    let requests = std::mem::take(&mut *world.resource_mut::<PrefabRequests>());

    if requests.open_toggle {
        crate::open_mode::toggle_open(world);
    }
    if requests.escape_close {
        crate::open_mode::request_close(world);
    }
    if requests.flatten {
        crate::open_mode::flatten_open(world);
    }
    if let Some(root_id) = requests.snap_after_move {
        snap_moved_instance(world, root_id);
    }
    if requests.repeat {
        repeat_piece(world);
    }
    if let Some(name) = world.resource_mut::<GroupCommit>().0.take() {
        match world.resource::<GroupPrompt>().purpose {
            PromptPurpose::Group => group_selection(world, name),
            PromptPurpose::Variant => make_variant(world, name),
        }
    }
    if requests.revert || requests.apply {
        let roots = selected_instance_roots(world);
        for root_id in roots {
            if requests.apply {
                apply_to_prefab(world, root_id);
            } else {
                revert_overrides(world, root_id);
            }
        }
    }
}

/// Selected instance roots (a selected stamped child resolves to its root).
fn selected_instance_roots(world: &mut World) -> Vec<SceneId> {
    let mut roots: Vec<SceneId> = {
        let mut query = world.query_filtered::<(
            Option<&StampedFrom>,
            Option<&PrefabInstance>,
            &SceneId,
        ), With<Selected>>();
        query
            .iter(world)
            .filter_map(|(stamped, instance, id)| {
                stamped.map(|s| s.instance_root).or(instance.map(|_| *id))
            })
            .collect()
    };
    roots.sort_by_key(|id| id.0);
    roots.dedup();
    roots
}

fn restamp(world: &mut World, root_id: SceneId) {
    let stamped: Vec<Entity> = {
        let mut query = world.query::<(Entity, &StampedFrom)>();
        query
            .iter(world)
            .filter(|(_, s)| s.instance_root == root_id)
            .map(|(e, _)| e)
            .collect()
    };
    for entity in stamped {
        // Recursive despawn may have taken descendants that are ALSO in the
        // list (hierarchical templates, nested instances) — guard, don't panic.
        if let Ok(entity) = world.get_entity_mut(entity) {
            entity.despawn();
        }
    }
    let Some(root) = world.resource::<SceneIndex>().get(&root_id) else {
        return;
    };
    // A root stamp_new_instances hasn't marked yet is NOT ours to stamp —
    // doing so double-stamps when a library bump lands the same frame as a
    // fresh spawn (found by the demo-kit generator: every part duplicated).
    if world.get::<crate::Stamped>(root).is_none() {
        return;
    }
    let Some(instance) = world.get::<PrefabInstance>(root).copied() else {
        return;
    };
    stamp_prefab(world, instance.0, root);
}

/// Revert: clear the deltas, restamp clean. (Undo nuance documented: the
/// override component Set is undoable, but the diff re-derives from restamped
/// state — revert is treated as a deliberate reset, not a history entry.)
fn revert_overrides(world: &mut World, root_id: SceneId) {
    let Some(root) = world.resource::<SceneIndex>().get(&root_id) else {
        return;
    };
    if let Some(mut overrides) = world.get_mut::<PrefabOverrides>(root) {
        overrides.0.clear();
    }
    restamp(world, root_id);
}

/// Fold this instance's deltas INTO the template, save, propagate everywhere.
fn apply_to_prefab(world: &mut World, root_id: SceneId) {
    let Some(root) = world.resource::<SceneIndex>().get(&root_id) else {
        return;
    };
    let Some(instance) = world.get::<PrefabInstance>(root).copied() else {
        return;
    };
    let patches: Vec<OverridePatch> = world
        .get::<PrefabOverrides>(root)
        .map(|o| o.0.clone())
        .unwrap_or_default();
    if patches.is_empty() {
        return;
    }
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    {
        let mut library = world.resource_mut::<PrefabLibrary>();
        let Some(prefab) = library.prefabs.get_mut(&instance.0) else {
            return;
        };
        // Rebuild the template with patches folded in.
        let records: Vec<(
            SceneId,
            Option<SceneId>,
            Vec<Box<dyn bevy::reflect::PartialReflect>>,
        )> = prefab
            .template
            .records()
            .map(|(id, parent, components)| {
                let components = components
                    .iter()
                    .map(|value| {
                        let mut dynamic = value.to_dynamic();
                        let type_path = value
                            .get_represented_type_info()
                            .map(|i| i.type_path())
                            .unwrap_or_default();
                        for patch in patches
                            .iter()
                            .filter(|p| p.entity == id.0.to_string() && p.type_path == type_path)
                        {
                            crate::overrides::apply_patch_value(
                                &registry,
                                dynamic.as_mut(),
                                &patch.path,
                                &patch.value,
                            );
                        }
                        dynamic
                    })
                    .collect();
                (id, parent, components)
            })
            .collect();
        prefab.template = snapshot_from_parts(records);
        prefab.generation_note();
    }
    if let Some(mut overrides) = world.get_mut::<PrefabOverrides>(root) {
        overrides.0.clear();
    }
    let def_snapshot = {
        let library = world.resource::<PrefabLibrary>();
        library.prefabs.get(&instance.0).map(|p| PrefabDef {
            id: p.id,
            name: p.name.clone(),
            template: snapshot_from_parts(
                p.template
                    .records()
                    .map(|(id, parent, c)| (id, parent, c.iter().map(|v| v.to_dynamic()).collect()))
                    .collect(),
            ),
        })
    };
    drop(registry);
    if let Some(def) = def_snapshot {
        save_prefab(world, &def);
    }
    world.resource_mut::<PrefabLibrary>().generation += 1;
}

/// Group the selection into a prefab (`g` + name): the selection is REPLACED in
/// place by an instance — one undoable transaction. Template transforms rebase
/// around the selection pivot so placed instances land where the cursor points.
pub(crate) fn group_selection(world: &mut World, name: String) {
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    let components = world.resource::<EditorComponents>().types.clone();
    let selected: Vec<(Entity, SceneId)> = {
        let mut query =
            world.query_filtered::<(Entity, &SceneId), (With<Selected>, Without<PrefabStamped>)>();
        let mut all: Vec<_> = query.iter(world).map(|(e, id)| (e, *id)).collect();
        all.sort_by_key(|(_, id)| id.0);
        all
    };
    if selected.is_empty() {
        return;
    }
    let name = if name.trim().is_empty() {
        "Prefab".to_string()
    } else {
        name.trim().to_string()
    };
    let selected_ids: std::collections::HashSet<SceneId> =
        selected.iter().map(|(_, id)| *id).collect();
    // Pivot: average translation of top-level members (parent not in selection).
    let mut pivot = Vec3::ZERO;
    let mut top_level = 0usize;
    for (entity, _) in &selected {
        let parent_in_selection = world
            .get::<ChildOf>(*entity)
            .and_then(|c| world.get::<SceneId>(c.parent()))
            .is_some_and(|p| selected_ids.contains(p));
        if !parent_in_selection && let Some(transform) = world.get::<Transform>(*entity) {
            pivot += transform.translation;
            top_level += 1;
        }
    }
    if top_level > 0 {
        pivot /= top_level as f32;
    }
    // Ground-project the pivot: members keep their heights, so an instance
    // placed at a ground point sits ON the ground, not sunk to half-height.
    pivot.y = 0.0;
    let records: Vec<(
        SceneId,
        Option<SceneId>,
        Vec<Box<dyn bevy::reflect::PartialReflect>>,
    )> = selected
        .iter()
        .map(|(entity, id)| {
            let parent = world
                .get::<ChildOf>(*entity)
                .and_then(|c| world.get::<SceneId>(c.parent()))
                .copied()
                .filter(|p| selected_ids.contains(p));
            let values = components
                .iter()
                .filter_map(|reg| {
                    let reflect_component = registry
                        .get(reg.type_id)?
                        .data::<bevy::ecs::reflect::ReflectComponent>()?;
                    let entity_ref = world.get_entity(*entity).ok()?;
                    let value = reflect_component.reflect(entity_ref)?;
                    // Top-level members rebase around the pivot.
                    if parent.is_none()
                        && let Some(transform) =
                            value.as_partial_reflect().try_downcast_ref::<Transform>()
                    {
                        let mut rebased = *transform;
                        rebased.translation -= pivot;
                        return Some(Box::new(rebased).into_partial_reflect());
                    }
                    Some(value.to_dynamic())
                })
                .collect();
            (*id, parent, values)
        })
        .collect();
    drop(registry);
    let def = PrefabDef {
        id: Uuid::new_v4(),
        name,
        template: snapshot_from_parts(records),
    };
    save_prefab(world, &def);
    let prefab_id = def.id;
    let prefab_name = def.name.clone();
    let count = def.template.records().count();
    world
        .resource_mut::<PrefabLibrary>()
        .prefabs
        .insert(def.id, def);
    world.resource_mut::<PrefabLibrary>().generation += 1;

    // Replace the selection with an instance — ONE undoable transaction.
    let root_id = SceneId::random();
    let mut ops: Vec<Op> = selected
        .iter()
        .map(|(_, id)| Op::Despawn { id: *id })
        .collect();
    ops.push(Op::Spawn {
        id: root_id,
        components: vec![
            Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
            Box::new(PrefabOverrides::default()).into_partial_reflect(),
            Box::new(Transform::from_translation(pivot)).into_partial_reflect(),
            Box::new(Name::new(prefab_name.clone())).into_partial_reflect(),
        ],
    });
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: format!("Group into '{prefab_name}'"),
        gesture: None,
        ops,
    });
    world.resource_mut::<PendingGroupSelect>().0 = Some(root_id);
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("◆ {prefab_name} created ({count} entities) — grouped in place"),
        success: true,
    });
}

/// Select the new instance once its spawn applied (next frame).
pub(crate) fn select_grouped(
    mut pending: ResMut<PendingGroupSelect>,
    index: Res<SceneIndex>,
    previous: Query<Entity, With<Selected>>,
    mut changed: MessageWriter<SelectionChanged>,
    mut commands: Commands,
) {
    let Some(root_id) = pending.0 else { return };
    let Some(entity) = index.get(&root_id) else {
        return;
    };
    pending.0 = None;
    for entity in &previous {
        commands.entity(entity).remove::<Selected>();
    }
    commands.entity(entity).insert(Selected);
    changed.write(SelectionChanged);
}

/// D6 variants: a variant is an ordinary prefab whose template is ONE record —
/// an instance of the base carrying the captured override deltas. Inheritance
/// and propagation then fall out of nested stamping: base edits restamp every
/// variant instance, variant deltas re-apply on top.
fn make_variant(world: &mut World, name: String) {
    let Some(root_id) = selected_instance_roots(world).first().copied() else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select a prefab instance to make a variant of".into(),
            success: false,
        });
        return;
    };
    let Some(root) = world.resource::<SceneIndex>().get(&root_id) else {
        return;
    };
    let Some(base) = world.get::<PrefabInstance>(root).copied() else {
        return;
    };
    let deltas = world
        .get::<PrefabOverrides>(root)
        .cloned()
        .unwrap_or_default();
    let transform = world.get::<Transform>(root).copied().unwrap_or_default();
    let base_name = world
        .resource::<PrefabLibrary>()
        .prefabs
        .get(&base.0)
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "prefab".into());

    let variant_id = Uuid::new_v4();
    let def = PrefabDef {
        id: variant_id,
        name: name.clone(),
        template: snapshot_from_parts(vec![(
            SceneId::random(),
            None,
            vec![
                Box::new(base).into_partial_reflect(),
                Box::new(deltas).into_partial_reflect(),
                Box::new(Transform::default()).into_partial_reflect(),
                Box::new(Name::new(base_name.clone())).into_partial_reflect(),
            ],
        )]),
    };
    save_prefab(world, &def);
    world
        .resource_mut::<PrefabLibrary>()
        .prefabs
        .insert(variant_id, def);
    world.resource_mut::<PrefabLibrary>().generation += 1;

    // Replace the source instance in place, one undoable transaction.
    let new_root = SceneId::random();
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: format!("Make Variant {name}"),
        gesture: None,
        ops: vec![
            Op::Despawn { id: root_id },
            Op::Spawn {
                id: new_root,
                components: vec![
                    Box::new(PrefabInstance(variant_id)).into_partial_reflect(),
                    Box::new(PrefabOverrides::default()).into_partial_reflect(),
                    Box::new(transform).into_partial_reflect(),
                    Box::new(Name::new(name.clone())).into_partial_reflect(),
                ],
            },
        ],
    });
    world.resource_mut::<PendingGroupSelect>().0 = Some(new_root);
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("\u{25c6} {name} — variant of {base_name}; base edits propagate"),
        success: true,
    });
}

/// D10 `o`: chain ANOTHER instance of the selected piece at its first FREE
/// socket (one not already mated to a socket within 5cm) — `o o o` runs a
/// wall. The new instance is selected, so the chain continues from the end.
fn repeat_piece(world: &mut World) {
    let Some(root_id) = selected_instance_roots(world).first().copied() else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select a prefab instance to repeat".into(),
            success: false,
        });
        return;
    };
    let Some(root) = world.resource::<SceneIndex>().get(&root_id) else {
        return;
    };
    let Some(instance) = world.get::<PrefabInstance>(root).copied() else {
        return;
    };
    let (name, def_sockets) = {
        let library = world.resource::<PrefabLibrary>();
        let Some(def) = library.prefabs.get(&instance.0) else {
            return;
        };
        (def.name.clone(), crate::sockets::template_sockets(def))
    };
    if def_sockets.is_empty() {
        world.write_message(editor_scene::SceneIoFeedback {
            message: format!("{name} has no sockets to chain from"),
            success: false,
        });
        return;
    }
    let members: Vec<Entity> = crate::open_mode::members_of(world, root);
    let mut own_sockets: Vec<(Entity, GlobalTransform, Transform)> = Vec::new();
    for member in &members {
        if world.get::<crate::sockets::Socket>(*member).is_some()
            && let (Some(global), Some(local)) = (
                world.get::<GlobalTransform>(*member).copied(),
                world.get::<Transform>(*member).copied(),
            )
        {
            own_sockets.push((*member, global, local));
        }
    }
    let other_positions: Vec<Vec3> = {
        let mut query = world.query::<(Entity, &GlobalTransform, &crate::sockets::Socket)>();
        query
            .iter(world)
            .filter(|(e, _, _)| !members.contains(e))
            .map(|(_, g, _)| g.translation())
            .collect()
    };
    let Some((_, exit_world, exit_local)) = own_sockets.iter().find(|(_, global, _)| {
        !other_positions
            .iter()
            .any(|p| p.distance(global.translation()) < 0.05)
    }) else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: format!("{name}: every socket already mated"),
            success: false,
        });
        return;
    };
    // Entry = a template socket that is NOT the exit's frame when possible
    // (walls: exit east, enter west — the piece EXTENDS instead of stacking).
    let entry = def_sockets
        .iter()
        .find(|(local, _)| local.translation.distance(exit_local.translation) > 0.05)
        .or_else(|| def_sockets.first())
        .unwrap();
    let new_root = crate::sockets::mate_transform(exit_world, &entry.0);
    let id = SceneId::random();
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: format!("Repeat {name}"),
        gesture: None,
        ops: vec![Op::Spawn {
            id,
            components: vec![
                Box::new(PrefabInstance(instance.0)).into_partial_reflect(),
                Box::new(PrefabOverrides::default()).into_partial_reflect(),
                Box::new(new_root).into_partial_reflect(),
                Box::new(Name::new(name.clone())).into_partial_reflect(),
            ],
        }],
    });
    world.resource_mut::<PendingGroupSelect>().0 = Some(id);
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("chained {name} — o again to continue"),
        success: true,
    });
}

/// D10: after a move-gesture commit on an instance, mate it with the nearest
/// compatible socket within reach — excluding its OWN sockets (no self-snap).
/// The correction is one plain undoable Set on top of the gesture's entry.
fn snap_moved_instance(world: &mut World, root_id: SceneId) {
    let Some(root) = world.resource::<SceneIndex>().get(&root_id) else {
        return;
    };
    let Some(instance) = world.get::<PrefabInstance>(root).copied() else {
        return;
    };
    let at = world
        .get::<Transform>(root)
        .map(|t| t.translation)
        .unwrap_or_default();
    let def_sockets = {
        let library = world.resource::<PrefabLibrary>();
        let Some(def) = library.prefabs.get(&instance.0) else {
            return;
        };
        crate::sockets::template_sockets(def)
    };
    // Exclude the moved instance's own stamped sockets from candidates by
    // masking them out for the query pass.
    let own: Vec<Entity> = crate::open_mode::members_of(world, root);
    let masked: Vec<Entity> = own
        .into_iter()
        .filter(|e| world.get::<crate::sockets::Socket>(*e).is_some())
        .collect();
    let saved: Vec<(Entity, crate::sockets::Socket)> = masked
        .iter()
        .filter_map(|e| {
            world
                .get::<crate::sockets::Socket>(*e)
                .cloned()
                .map(|s| (*e, s))
        })
        .collect();
    for (entity, _) in &saved {
        world.entity_mut(*entity).remove::<crate::sockets::Socket>();
    }
    let snap = crate::sockets::snap_for_placement(world, &def_sockets, at, 2.0);
    for (entity, socket) in saved {
        world.entity_mut(entity).insert(socket);
    }
    let Some((transform, label)) = snap else {
        return;
    };
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: "Snap To Socket".into(),
        gesture: None,
        ops: vec![Op::Set {
            target: root_id,
            value: Box::new(transform).into_partial_reflect(),
        }],
    });
    world.write_message(editor_scene::SceneIoFeedback {
        message: label,
        success: true,
    });
}

/// Generation-driven propagation: any library change restamps every instance
/// (patches re-apply, so overrides survive — D5's propagation contract).
pub(crate) fn restamp_on_library_change(world: &mut World) {
    let generation = world.resource::<PrefabLibrary>().generation;
    let last = world.resource::<LastRestampedGeneration>().0;
    if generation == last {
        return;
    }
    world.resource_mut::<LastRestampedGeneration>().0 = generation;
    let roots: Vec<SceneId> = {
        let mut query = world.query_filtered::<&SceneId, With<PrefabInstance>>();
        query.iter(world).copied().collect()
    };
    for root_id in roots {
        restamp(world, root_id);
    }
    let _ = world; // markers: Stamped roots keep their marker; restamp replaced children
}

#[derive(Resource, Default)]
pub(crate) struct LastRestampedGeneration(pub u64);
