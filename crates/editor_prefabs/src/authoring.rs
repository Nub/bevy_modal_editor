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

/// Prefabs live in the ASSET tree, alongside the models and textures they
/// reference — one content root that ships with the game, rather than a
/// directory that only exists next to wherever the editor was launched from.
pub const PREFABS_DIR: &str = "prefabs";

/// The resolved prefab library directory: `<assets>/prefabs`, keyed off the
/// SAME root the AssetServer serves (`editor_scene::models::assets_fs_root`),
/// so prefabs and the models they reference can never drift apart.
pub fn prefabs_dir() -> PathBuf {
    editor_scene::models::assets_fs_root().join(PREFABS_DIR)
}

#[derive(Resource, Default)]
pub(crate) struct PrefabRequests {
    revert: bool,
    apply: bool,
    open_toggle: bool,
    escape_close: bool,
    flatten: bool,
    /// Instance root to socket-snap after a move gesture commits (D10).
    snap_after_move: Option<SceneId>,
    /// Snap the selected socket onto its piece's bounds (owner ask).
    snap_socket: Option<crate::sockets::SnapFeature>,
    /// Generate sockets on the selection's faces (owner ask).
    generate_sockets: Option<crate::sockets::SocketSides>,
    /// Pin the selected socket as the chain's IN end (owner ask).
    pin_chain_entry: bool,
    repeat: bool,
    /// Enter socket mode, arming a socket to work from (owner ask).
    arm_socket: bool,
    /// Move to the next socket round the armed piece.
    next_socket: bool,
    /// Leave socket mode.
    exit_socket: bool,
    /// Open the palette to place a piece ON the armed socket.
    insert_at_socket: bool,
}

/// What the inline name prompt is naming (title + commit routing).
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum PromptPurpose {
    #[default]
    Group,
    Variant,
    Kit,
    /// Not a name at all — a COUNT: how many pieces to lay in the run.
    Fill,
    /// Not a prefab flow: the ONE name prompt serves every rename, and the
    /// owner of the purpose consumes the commit (see `perform_prefab_requests`,
    /// which deliberately leaves non-prefab commits alone).
    RenameMaterial,
    /// Naming a timeline event. An event's whole content IS its name — it is
    /// what the game matches on — so it is asked for rather than generated.
    TimelineEvent,
}

impl PromptPurpose {
    /// Whether the prefab performer owns this prompt's committed name.
    pub fn is_prefab_flow(self) -> bool {
        matches!(self, Self::Group | Self::Variant | Self::Kit | Self::Fill)
    }
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
            "prefab.set-kit" => {
                if !selection.is_empty() {
                    prompt.open = true;
                    prompt.purpose = PromptPurpose::Kit;
                }
            }
            "prefab.revert-overrides" => requests.revert = true,
            "prefab.apply-to-prefab" => requests.apply = true,
            "prefab.open" => requests.open_toggle = true,
            "prefab.flatten" => requests.flatten = true,
            "prefab.bake" => bake_requests.bake = true,
            "prefab.repeat" => requests.repeat = true,
            "socket.arm" => requests.arm_socket = true,
            "socket.next" => requests.next_socket = true,
            "socket.exit" => requests.exit_socket = true,
            "socket.insert" => requests.insert_at_socket = true,
            "prefab.fill" => {
                if !selection.is_empty() {
                    prompt.open = true;
                    prompt.purpose = PromptPurpose::Fill;
                }
            }
            "socket.snap-face" => requests.snap_socket = Some(crate::sockets::SnapFeature::Face),
            "socket.snap-edge" => requests.snap_socket = Some(crate::sockets::SnapFeature::Edge),
            "socket.snap-corner" => {
                requests.snap_socket = Some(crate::sockets::SnapFeature::Corner)
            }
            "socket.generate-ends" => {
                requests.generate_sockets = Some(crate::sockets::SocketSides::Ends)
            }
            "socket.generate-sides" => {
                requests.generate_sockets = Some(crate::sockets::SocketSides::Sides)
            }
            "socket.generate-all" => {
                requests.generate_sockets = Some(crate::sockets::SocketSides::All)
            }
            "chain.set-in" => requests.pin_chain_entry = true,
            // D10: when a move gesture commits on a single prefab instance,
            // try to mate it with a nearby compatible socket. Collect runs
            // pre-conventions, so the gesture is still Active here.
            "transform.commit" => {
                if let MoveGesture::Active {
                    // MOVE only, exactly as `snap_during_drag` filters: mating
                    // replaces the whole transform, so letting a rotate or a
                    // scale commit through here would throw away the angle or
                    // the size the user just dialled in — and as a separate
                    // history entry, so one undo could not put it back.
                    kind: editor_core::gesture::GestureKind::Move,
                    originals,
                    ..
                } = &*gesture
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
    let Ok(entries) = std::fs::read_dir(prefabs_dir()) else {
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
    let _ = std::fs::create_dir_all(prefabs_dir());
    let path = prefabs_dir().join(format!(
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
    if requests.arm_socket {
        enter_socket_mode(world);
    }
    if requests.next_socket {
        cycle_socket(world);
    }
    if requests.exit_socket {
        leave_socket_mode(world);
    }
    if requests.insert_at_socket {
        insert_at_armed_socket(world);
    }
    if let Some(feature) = requests.snap_socket {
        snap_selected_sockets(world, feature);
    }
    if let Some(sides) = requests.generate_sockets {
        generate_sockets(world, sides);
    }
    if requests.pin_chain_entry {
        pin_chain_entry(world);
    }
    // Another feature's rename must survive this pass untouched — taking the
    // commit here would swallow it before its owner ever sees it.
    if world.resource::<GroupPrompt>().purpose.is_prefab_flow()
        && let Some(name) = world.resource_mut::<GroupCommit>().0.take()
    {
        match world.resource::<GroupPrompt>().purpose {
            PromptPurpose::Group => group_selection(world, name),
            PromptPurpose::Variant => make_variant(world, name),
            PromptPurpose::Kit => set_kit(world, name),
            // A count, not a name — anything unparsable lays nothing rather
            // than guessing a number.
            PromptPurpose::Fill => match name.trim().parse::<usize>() {
                Ok(count) if count > 0 => fill_run(world, count),
                _ => {
                    world.write_message(editor_scene::SceneIoFeedback {
                        message: format!("fill needs a count, got {name:?}"),
                        success: false,
                    });
                }
            },
            // Not prefab flows: their owners consume the commit. Unreachable
            // anyway, since the guard above admits only prefab purposes.
            PromptPurpose::RenameMaterial | PromptPurpose::TimelineEvent => {}
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
pub(crate) fn selected_instance_roots(world: &mut World) -> Vec<SceneId> {
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
            kit: p.kit.clone(),
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
    let roots: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, (With<Selected>, Without<PrefabStamped>)>();
        query.iter(world).collect()
    };
    // Selection CLOSURE: selecting a parent means the THING, subtree included.
    // Capturing only the selected entities would stamp a hollow template while
    // the replace-despawn recursively deleted the uncaptured children.
    // (depth rides along: despawn ops must run children-first so every
    // member's inverse is captured for undo.)
    let mut selected: Vec<(Entity, SceneId, usize)> = Vec::new();
    let mut seen: std::collections::HashSet<SceneId> = Default::default();
    let mut stack: Vec<(Entity, usize)> = roots.into_iter().map(|e| (e, 0)).collect();
    while let Some((entity, depth)) = stack.pop() {
        let Some(id) = world.get::<SceneId>(entity).copied() else {
            continue; // derived subtrees (gltf spawns) stay derived
        };
        if !seen.insert(id) {
            continue;
        }
        selected.push((entity, id, depth));
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter().map(|c| (c, depth + 1)));
        }
    }
    let mut despawn_order = selected.clone();
    despawn_order.sort_by(|a, b| b.2.cmp(&a.2)); // deepest first
    let selected: Vec<(Entity, SceneId)> = {
        let mut all: Vec<_> = selected.into_iter().map(|(e, id, _)| (e, id)).collect();
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
        kit: None,
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
    // Children despawn BEFORE parents: a recursive parent despawn would kill
    // them without an inverse, and undo would restore a hollow subtree.
    let root_id = SceneId::random();
    debug!(
        "group '{prefab_name}': despawning {:?}",
        despawn_order
            .iter()
            .map(|(_, id, depth)| (*id, *depth))
            .collect::<Vec<_>>()
    );
    let mut ops: Vec<Op> = despawn_order
        .iter()
        .map(|(_, id, _)| Op::Despawn { id: *id })
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
        kit: None,
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

/// D10: tag the selected instance's PREFAB with a kit name (empty clears).
/// Kit membership drives coherence checks; saved with the prefab file.
fn set_kit(world: &mut World, name: String) {
    let Some(root_id) = selected_instance_roots(world).first().copied() else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select a prefab instance to set its kit".into(),
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
    let trimmed = name.trim().to_string();
    {
        let mut library = world.resource_mut::<PrefabLibrary>();
        let Some(def) = library.prefabs.get_mut(&instance.0) else {
            return;
        };
        def.kit = (!trimmed.is_empty()).then_some(trimmed.clone());
    }
    let def_clone = crate::open_mode::clone_def(world, instance.0);
    if let Some(def) = def_clone {
        save_prefab_public(world, &def);
        world.resource_mut::<PrefabLibrary>().generation += 1;
        world.write_message(editor_scene::SceneIoFeedback {
            message: if trimmed.is_empty() {
                format!("{} removed from its kit", def.name)
            } else {
                format!("{} joined kit \u{201c}{trimmed}\u{201d}", def.name)
            },
            success: true,
        });
    }
}

/// D10 `o`: chain ANOTHER instance of the selected piece at its first FREE
/// socket (one not already mated to a socket within 5cm) — `o o o` runs a
/// wall. The new instance is selected, so the chain continues from the end.
/// `prefab.fill`: lay a whole RUN in one go. The chain step between identical
/// pieces is a constant rigid transform, so the first mate defines it and the
/// rest is `step` applied repeatedly — one transaction, one undo entry, no
/// pressing `o` thirty times.
///
/// `count` is how many NEW pieces to add. Direction comes from the same place
/// as `o`: the socket you picked, else away from where the run came in.
fn fill_run(world: &mut World, count: usize) {
    let before = query_instance_ids(world);
    // The first piece goes through the ordinary chain path, so fill and `o`
    // can never disagree about direction, mating or orientation.
    let chained_from = repeat_piece(world);
    apply_pending(world);
    let after = query_instance_ids(world);
    let Some(first) = after.iter().find(|id| !before.contains(id)).copied() else {
        return; // repeat_piece already explained why it could not chain
    };
    // The step is measured from the piece we ACTUALLY chained off. Taking the
    // last id an unordered query happened to return was right only in a scene
    // holding exactly one instance — which is precisely what the fixture held,
    // so the test passed while real scenes scattered.
    let Some(source) = chained_from else {
        return;
    };
    let index = world.resource::<SceneIndex>();
    let (Some(source_entity), Some(first_entity)) = (index.get(&source), index.get(&first)) else {
        return;
    };
    let (Some(from), Some(to)) = (
        world.get::<Transform>(source_entity).copied(),
        world.get::<Transform>(first_entity).copied(),
    ) else {
        return;
    };
    // step = from⁻¹ · to, in the parent frame both share.
    let step = from.compute_affine().inverse() * to.compute_affine();
    let prefab = world.get::<PrefabInstance>(first_entity).copied();
    let name = world
        .get::<Name>(first_entity)
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| "piece".into());
    let (Some(prefab), true) = (prefab, count > 1) else {
        return;
    };
    let mut ops = Vec::new();
    let mut pose = to.compute_affine();
    for _ in 1..count {
        pose *= step;
        ops.push(Op::Spawn {
            id: SceneId::random(),
            components: vec![
                Box::new(prefab).into_partial_reflect(),
                Box::new(PrefabOverrides::default()).into_partial_reflect(),
                Box::new(Transform::from_matrix(pose.into())).into_partial_reflect(),
                Box::new(Name::new(name.clone())).into_partial_reflect(),
            ],
        });
    }
    let laid = ops.len();
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: format!("Fill {name}"),
        gesture: None,
        ops,
    });
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("filled a run of {} {name}", laid + 1),
        success: true,
    });
}

fn query_instance_ids(world: &mut World) -> Vec<SceneId> {
    let mut query =
        world.query_filtered::<&SceneId, (With<PrefabInstance>, Without<PrefabStamped>)>();
    query.iter(world).copied().collect()
}

/// Flush the queued transaction so the chained piece EXISTS before the run
/// measures its step (the fill is one pass, not one per frame).
fn apply_pending(world: &mut World) {
    editor_core::edits::apply_edits(world);
}

/// The chain's IN end: which socket of the NEXT piece mates onto the one you
/// grow from. Held per prefab, because the frame only means anything for the
/// piece it came off — and cleared the moment you chain a different one.
#[derive(Resource, Default)]
pub struct ChainEntry {
    pub prefab: Option<Uuid>,
    /// Root-relative frame of the chosen socket.
    pub local: Option<Transform>,
}

/// `chain.set-in`: pin the selected socket as the end the next piece arrives
/// by. Picking the OUT socket steers direction; this steers the new piece's
/// ORIENTATION, which is the other half of "which socket to which".
fn pin_chain_entry(world: &mut World) {
    let Some((socket, root_id)) = selected_chain_socket(world) else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select a socket to pin as the chain's in end".into(),
            success: false,
        });
        return;
    };
    let prefab = world
        .resource::<SceneIndex>()
        .get(&root_id)
        .and_then(|root| world.get::<PrefabInstance>(root).map(|i| i.0));
    let local = world.get::<Transform>(socket).copied();
    let name = world
        .get::<Name>(socket)
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| "socket".into());
    *world.resource_mut::<ChainEntry>() = ChainEntry { prefab, local };
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("chain enters by {name}"),
        success: true,
    });
}

/// A SELECTED socket is a direction: chain out of THAT one. Returns the socket
/// and the instance root owning it, so picking a socket is enough to say both
/// "repeat this piece" and "grow this way".
/// `o` — enter SOCKET MODE and arm a socket to work from.
///
/// The owner's grammar: "make `o` a mode, tab or clicking a socket selects,
/// then `i` inserts a new object on that socket." So `o` no longer places a
/// piece by itself; it puts you where placing happens, with a socket armed and
/// the keys that matter rebound. Modal editing answers "does this break `i`?"
/// by rebinding `i`: in socket mode it means place a piece HERE.
fn enter_socket_mode(world: &mut World) {
    let already_armed = {
        let mut query =
            world.query_filtered::<Entity, (With<crate::sockets::Socket>, With<Selected>)>();
        query.iter(world).next().is_some()
    };
    if !already_armed {
        cycle_socket(world);
    }
    let armed = {
        let mut query =
            world.query_filtered::<Entity, (With<crate::sockets::Socket>, With<Selected>)>();
        query.iter(world).next()
    };
    if armed.is_none() {
        return; // cycle_socket already said why
    }
    world
        .resource_mut::<editor_core::prelude::OverlayContext>()
        .0 = Some(crate::sockets::SOCKET_CONTEXT);
    world.write_message(editor_scene::SceneIoFeedback {
        message: "socket mode \u{b7} tab next \u{b7} i place \u{b7} o chain \u{b7} esc done".into(),
        success: true,
    });
}

fn leave_socket_mode(world: &mut World) {
    world
        .resource_mut::<editor_core::prelude::OverlayContext>()
        .0 = None;
    world.write_message(editor_scene::SceneIoFeedback {
        message: "socket mode off".into(),
        success: true,
    });
}

/// `i` inside socket mode: the ordinary insert palette, which already mates
/// what you pick to the armed socket.
fn insert_at_armed_socket(world: &mut World) {
    // Leaving the overlay first is what lets the palette's own keys (typing,
    // Enter, Escape) work — the layer exists to rebind `i`, not to sit on top
    // of the surface `i` opens.
    world
        .resource_mut::<editor_core::prelude::OverlayContext>()
        .0 = None;
    let changed = {
        let mut mode = world.resource_mut::<editor_core::prelude::CurrentMode>();
        (mode.0 != editor_core::prelude::MODE_INSERT)
            .then(|| std::mem::replace(&mut mode.0, editor_core::prelude::MODE_INSERT))
    };
    if let Some(from) = changed {
        world.write_message(editor_core::prelude::ModeChanged {
            from,
            to: editor_core::prelude::MODE_INSERT,
        });
    }
}

/// `socket.next` — ARM the socket the next piece will use.
///
/// The owner's ask, in their words: "there needs to be a way with `o` to
/// determine which socket will be used", and "tab or clicking a socket selects,
/// then `i` inserts a new object on that socket".
///
/// So a socket is ARMED by selecting it, and everything downstream already
/// honours that: `i` places the next piece mated to it, `o` chains from it, and
/// a rotate pivots about it. Tab is how you say it without aiming — with a
/// PIECE selected it arms that piece's first free socket, and with a socket
/// already armed it moves to the next one round the piece, wrapping.
///
/// Deliberately NOT an exclusive keyboard mode: an overlay context grabs every
/// key, and while a socket is armed you still want `i`, `o`, Escape and the
/// move gesture to mean what they always mean. The mode-feel comes from the
/// armed socket being visible and named, not from taking the keyboard away.
fn cycle_socket(world: &mut World) {
    let selected: Vec<Entity> = world
        .query_filtered::<Entity, With<Selected>>()
        .iter(world)
        .collect();
    let armed = selected
        .iter()
        .copied()
        .find(|entity| world.get::<crate::sockets::Socket>(*entity).is_some());
    // The piece whose sockets we are cycling: the armed socket's owner, or the
    // selected piece itself.
    let piece = match armed {
        Some(socket) => socket_owner_entity(world, socket),
        None => selected
            .iter()
            .copied()
            .find(|entity| world.get::<SceneId>(*entity).is_some()),
    };
    let Some(piece) = piece else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select a piece (or one of its sockets) first".into(),
            success: false,
        });
        return;
    };
    let sockets = sockets_of_piece(world, piece);
    if sockets.is_empty() {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "this piece has no sockets — space s 2 gives it some".into(),
            success: false,
        });
        return;
    }
    let next = match armed {
        Some(current) => {
            let at = sockets.iter().position(|entity| *entity == current);
            sockets[(at.unwrap_or(0) + 1) % sockets.len()]
        }
        None => sockets[0],
    };
    for previous in selected {
        world.entity_mut(previous).remove::<Selected>();
    }
    world.entity_mut(next).insert(Selected);
    world.write_message(editor_core::selection::SelectionChanged);
    let name = world
        .get::<crate::sockets::Socket>(next)
        .map(|socket| socket.name.clone())
        .unwrap_or_else(|| "socket".into());
    let index = sockets
        .iter()
        .position(|entity| *entity == next)
        .unwrap_or(0)
        + 1;
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!(
            "socket {name} ({index}/{}) \u{b7} i places here \u{b7} o chains",
            sockets.len()
        ),
        success: true,
    });
}

/// Sockets under a piece, in a STABLE order, so Tab walks the same ring every
/// time rather than a hash order that changes between runs.
fn sockets_of_piece(world: &mut World, piece: Entity) -> Vec<Entity> {
    // `members_of`, not a Children walk: a prefab instance's stamped members
    // are not necessarily its children, and walking the hierarchy found nothing
    // on exactly the pieces this verb is for.
    let members = crate::open_mode::members_of(world, piece);
    let mut found: Vec<(Entity, Vec3)> = Vec::new();
    for entity in members {
        if world.get::<crate::sockets::Socket>(entity).is_some()
            && let Some(global) = world.get::<GlobalTransform>(entity)
        {
            found.push((entity, global.translation()));
        }
    }
    found.sort_by(|a, b| {
        a.1.x
            .total_cmp(&b.1.x)
            .then(a.1.z.total_cmp(&b.1.z))
            .then(a.1.y.total_cmp(&b.1.y))
    });
    found.into_iter().map(|(entity, _)| entity).collect()
}

/// The piece a socket hangs off — an instance root, a stamped member's root, or
/// simply the nearest ancestor that is scene content.
fn socket_owner_entity(world: &World, socket: Entity) -> Option<Entity> {
    let mut current = socket;
    while let Some(parent) = world.get::<ChildOf>(current).map(|parent| parent.parent()) {
        if world.get::<SceneId>(parent).is_some() {
            return Some(parent);
        }
        current = parent;
    }
    None
}

fn selected_chain_socket(world: &mut World) -> Option<(Entity, SceneId)> {
    let socket = {
        let mut query =
            world.query_filtered::<Entity, (With<crate::sockets::Socket>, With<Selected>)>();
        query.iter(world).next()?
    };
    // Walk up to the instance root that owns it (adopted sockets hang off the
    // root; stamped ones name it directly).
    let mut current = socket;
    loop {
        if world.get::<PrefabInstance>(current).is_some()
            && let Some(id) = world.get::<SceneId>(current)
        {
            return Some((socket, *id));
        }
        if let Some(stamped) = world.get::<StampedFrom>(current) {
            return Some((socket, stamped.instance_root));
        }
        current = world.get::<ChildOf>(current).map(|c| c.parent())?;
    }
}

/// Chains one more piece off the selection, returning the instance it chained
/// FROM — `fill_run` needs exactly that to measure its step, and deriving it
/// again from a world query cannot tell which instance was the source.
fn repeat_piece(world: &mut World) -> Option<SceneId> {
    // Picking a socket picks the direction; otherwise the selected instance
    // chains from whichever free socket leads away from the run.
    let chosen = selected_chain_socket(world);
    let Some(root_id) = chosen
        .map(|(_, root)| root)
        .or_else(|| selected_instance_roots(world).first().copied())
    else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select a prefab instance (or one of its sockets) to repeat".into(),
            success: false,
        });
        return None;
    };
    let root = world.resource::<SceneIndex>().get(&root_id)?;
    let instance = world.get::<PrefabInstance>(root).copied()?;
    let (name, def_sockets) = {
        let library = world.resource::<PrefabLibrary>();
        let def = library.prefabs.get(&instance.0)?;
        (def.name.clone(), crate::sockets::template_sockets(def))
    };
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
    // An explicitly picked OUT socket wins outright — that is the whole point
    // of picking one.
    let picked_exit = chosen.and_then(|(socket, _)| {
        own_sockets
            .iter()
            .find(|(entity, _, _)| *entity == socket)
            .map(|(_, global, local)| (*global, *local))
    });
    let occupied: Vec<Vec3> = own_sockets
        .iter()
        .filter(|(_, global, _)| {
            other_positions
                .iter()
                .any(|p| p.distance(global.translation()) < 0.05)
        })
        .map(|(_, _, local)| local.translation)
        .collect();
    // Continue AWAY from where the chain came in: among the free sockets, take
    // the one farthest from an already-mated one. On a two-socket wall that is
    // the far end, on a four-socket tile the opposite face — either way a
    // straight run instead of a piece that wanders and climbs, which is what
    // "first free socket" produced.
    let auto_exit = own_sockets
        .iter()
        .filter(|(_, global, _)| {
            !other_positions
                .iter()
                .any(|p| p.distance(global.translation()) < 0.05)
        })
        .max_by(|(_, _, a), (_, _, b)| {
            let reach = |local: &Transform| {
                occupied
                    .iter()
                    .map(|at| at.distance(local.translation))
                    .fold(f32::MAX, f32::min)
            };
            reach(a).total_cmp(&reach(b))
        })
        .map(|(_, global, local)| (*global, *local));
    let Some((exit_world, exit_local)) = picked_exit.or(auto_exit) else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: format!("{name}: every socket already mated"),
            success: false,
        });
        return None;
    };
    // Entry comes from the sockets the piece ACTUALLY has, template or not:
    // sockets generated onto a placed instance live in the scene, and requiring
    // them to be template members meant "generate sockets, then chain" simply
    // reported that the piece had none. The template is the fallback for a
    // piece whose instance carries nothing of its own.
    let own_locals: Vec<Transform> = own_sockets.iter().map(|(_, _, local)| *local).collect();
    let candidates: Vec<Transform> = if own_locals.is_empty() {
        def_sockets.iter().map(|(local, _)| *local).collect()
    } else {
        own_locals
    };
    if candidates.is_empty() {
        world.write_message(editor_scene::SceneIoFeedback {
            message: format!("{name} has no sockets to chain from"),
            success: false,
        });
        return None;
    }
    // A pinned IN socket wins for this prefab; otherwise entry = a socket that
    // is NOT the exit's frame when possible (walls: exit east, enter west — the
    // piece EXTENDS instead of stacking).
    let pinned_entry = {
        let chain = world.resource::<ChainEntry>();
        (chain.prefab == Some(instance.0))
            .then_some(chain.local)
            .flatten()
    };
    let entry = pinned_entry.unwrap_or_else(|| {
        candidates
            .iter()
            .find(|local| local.translation.distance(exit_local.translation) > 0.05)
            .or_else(|| candidates.first())
            .copied()
            .unwrap()
    });
    let new_root = crate::sockets::mate_transform(&exit_world, &entry);
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
    Some(root_id)
}

/// D10: after a move-gesture commit on an instance, mate it with the nearest
/// compatible socket within reach — excluding its OWN sockets (no self-snap).
/// The correction is one plain undoable Set on top of the gesture's entry.
/// The piece's own geometry in ROOT-LOCAL space — sockets (and their gizmos)
/// excluded, or a socket already out at the edge would inflate the very bounds
/// it is being snapped to.
fn piece_bounds(world: &mut World, root: Entity) -> Option<(Vec3, Vec3)> {
    let to_local = world.get::<GlobalTransform>(root)?.affine().inverse();
    let mut min = Vec3::MAX;
    let mut max = Vec3::MIN;
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if entity != root && world.get::<crate::sockets::Socket>(entity).is_some() {
            continue; // a socket is not part of the shape
        }
        if let (Some(aabb), Some(global)) = (
            world.get::<bevy::camera::primitives::Aabb>(entity).copied(),
            world.get::<GlobalTransform>(entity).copied(),
        ) {
            let centre = Vec3::from(aabb.center);
            let half = Vec3::from(aabb.half_extents);
            for corner in 0..8 {
                let sign = Vec3::new(
                    if corner & 1 == 0 { -1.0 } else { 1.0 },
                    if corner & 2 == 0 { -1.0 } else { 1.0 },
                    if corner & 4 == 0 { -1.0 } else { 1.0 },
                );
                let local = to_local.transform_point3(global.transform_point(centre + half * sign));
                min = min.min(local);
                max = max.max(local);
            }
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    if min.x <= max.x {
        return Some((min, max));
    }
    // Nothing loaded yet. Before the Process stage ran at import this was the
    // end of the line: select a model you just imported, generate sockets, and
    // get NOTHING, with no message, because the gltf was still in flight. The
    // pipeline measured the same box at import time and the library is holding
    // it, so the answer exists — it just was not being asked for.
    imported_bounds(world, root)
}

/// The pre-load answer: bounds recorded by the `gltf.bounds` processor for
/// whichever imported model this subtree references.
///
/// Model-space, which is root-local here because the derived gltf subtree is
/// parented at identity — see `MeshRefDerived`.
fn imported_bounds(world: &mut World, root: Entity) -> Option<(Vec3, Vec3)> {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if let Some(mesh_ref) = world.get::<editor_scene::models::MeshRef>(entity).copied() {
            let bounds = world
                .resource::<editor_scene::models::ModelLibrary>()
                .get(&mesh_ref.0)
                .and_then(|entry| entry.bounds);
            if let Some(bounds) = bounds {
                return Some((Vec3::from(bounds.min), Vec3::from(bounds.max)));
            }
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    None
}

/// The socket type this piece should use: one it already has, else one used by
/// a piece of the same kit, else nothing.
///
/// Type is the kit's compatibility rule and it is invisible in the viewport —
/// two sockets of different types are two identical cones that will never mate
/// and never say so. Inheriting it is what makes "generate sockets and chain"
/// work at all on a piece added to an existing kit.
fn existing_type(world: &mut World, piece: Entity) -> Option<String> {
    // On the piece itself (or anywhere under it).
    let mut stack = vec![piece];
    while let Some(entity) = stack.pop() {
        if let Some(socket) = world.get::<crate::sockets::Socket>(entity) {
            return Some(socket.socket_type.clone());
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    // Otherwise: whatever this piece's KIT already mates with.
    let kit = world
        .get::<PrefabInstance>(piece)
        .and_then(|instance| world.resource::<PrefabLibrary>().prefabs.get(&instance.0))
        .and_then(|def| def.kit.clone())?;
    let sibling_types: Vec<String> = {
        let library = world.resource::<PrefabLibrary>();
        library
            .prefabs
            .values()
            .filter(|def| def.kit.as_ref() == Some(&kit))
            .flat_map(crate::sockets::template_sockets)
            .map(|(_, socket)| socket.socket_type)
            .collect()
    };
    sibling_types.into_iter().next()
}
/// `socket.generate-*`: put a socket on each chosen face of the selection's
/// bounds, centred and aimed outward — the fast path to a piece that actually
/// chains. A wall wants ends, a floor tile wants sides, and typing eight
/// transforms by hand to find that out is the reason kits go unbuilt.
///
/// Idempotent: a face that already has a socket near its centre is left alone,
/// so running it twice (or after hand-authoring one) never doubles up.
fn generate_sockets(world: &mut World, sides: crate::sockets::SocketSides) {
    let pieces: Vec<(Entity, SceneId)> = {
        let mut query = world.query_filtered::<
            (Entity, &SceneId),
            (With<Selected>, Without<crate::sockets::Socket>),
        >();
        query.iter(world).map(|(e, id)| (e, *id)).collect()
    };
    if pieces.is_empty() {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select a piece to generate sockets on".into(),
            success: false,
        });
        return;
    }
    let mut ops = Vec::new();
    let mut made = 0usize;
    // Named in the toast: the type is the compatibility rule and it is invisible
    // in the viewport, so the one moment it is knowable should say it.
    let mut last_type = String::from("default");
    for (piece, piece_id) in pieces {
        let Some((min, max)) = piece_bounds(world, piece) else {
            continue;
        };
        // INHERIT the type. Stamping "default" unconditionally is a silent,
        // permanent refusal to mate: kit pieces carry "wall", the two cones
        // look identical, and nothing anywhere says why the piece you just
        // socketed will not join the run. Take the type from a socket the piece
        // already has, else from a socket on any piece in the same kit, else
        // "default" — which is at least self-consistent for a fresh piece.
        let socket_type = existing_type(world, piece).unwrap_or_else(|| "default".into());
        last_type = socket_type.clone();
        // What is already there, so a re-run tops up rather than duplicates.
        let existing: Vec<Vec3> = world
            .get::<Children>(piece)
            .map(|children| {
                children
                    .iter()
                    .filter(|child| world.get::<crate::sockets::Socket>(*child).is_some())
                    .filter_map(|child| world.get::<Transform>(child).map(|t| t.translation))
                    .collect()
            })
            .unwrap_or_default();
        for (name, normal) in sides.faces() {
            let placed = crate::sockets::face_socket(min, max, *normal);
            let occupied = existing
                .iter()
                .any(|at| at.distance(placed.translation) < 0.05);
            if occupied {
                continue;
            }
            let id = SceneId::random();
            ops.push(Op::Spawn {
                id,
                components: vec![
                    Box::new(crate::sockets::Socket {
                        name: (*name).to_string(),
                        // INHERITED, not "default": see `existing_type`. One
                        // type across a generated set, so any two of them mate.
                        socket_type: socket_type.clone(),
                    })
                    .into_partial_reflect(),
                    Box::new(placed).into_partial_reflect(),
                    Box::new(Name::new(format!("socket {name}"))).into_partial_reflect(),
                ],
            });
            ops.push(Op::Reparent {
                target: id,
                parent: Some(piece_id),
            });
            made += 1;
        }
    }
    if made == 0 {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "nothing to add — those faces already have sockets (or the piece has no \
                      geometry yet)"
                .into(),
            success: false,
        });
        return;
    }
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: "Generate Sockets".into(),
        gesture: None,
        ops,
    });
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!(
            "generated {made} socket{} \u{b7} type {last_type}",
            if made == 1 { "" } else { "s" }
        ),
        success: true,
    });
}

/// The closest scene entity that HAS geometry, and the socket's position
/// expressed in that entity's local space. Sockets are never candidates (a
/// socket does not sit on another socket), nor is the socket's own subtree.
fn nearest_piece(world: &mut World, socket_id: SceneId) -> Option<(Entity, Transform)> {
    let socket = world.resource::<SceneIndex>().get(&socket_id)?;
    let socket_world = world.get::<GlobalTransform>(socket).copied()?;
    let candidates: Vec<Entity> = {
        let mut query =
            world.query_filtered::<Entity, (With<SceneId>, Without<crate::sockets::Socket>)>();
        query.iter(world).collect()
    };
    let mut best: Option<(f32, Entity)> = None;
    for candidate in candidates {
        if candidate == socket {
            continue;
        }
        let Some((min, max)) = piece_bounds(world, candidate) else {
            continue; // no geometry — not a piece
        };
        let Some(piece_world) = world.get::<GlobalTransform>(candidate).copied() else {
            continue;
        };
        // Distance to the bounds, not the origin: a long wall's origin can be
        // further away than a small prop the socket is nowhere near.
        let local = piece_world
            .affine()
            .inverse()
            .transform_point3(socket_world.translation());
        let distance = (local.clamp(min, max) - local).length();
        if best.is_none_or(|(closest, _)| distance < closest) {
            best = Some((distance, candidate));
        }
    }
    let (_, piece) = best?;
    // NEVER adopt into a stamped subtree: `restamp` despawns every
    // `StampedFrom` entity (recursively) on any library change, so a socket
    // parented to a member vanishes the next time anything saves a prefab —
    // "worked for a second, then stopped". The instance ROOT is not stamped,
    // survives restamping, and is the frame a socket belongs in anyway.
    let piece = match world.get::<StampedFrom>(piece).map(|s| s.instance_root) {
        Some(root_id) => world.resource::<SceneIndex>().get(&root_id)?,
        None => piece,
    };
    let piece_world = world.get::<GlobalTransform>(piece).copied()?;
    // The socket keeps its WORLD pose through the adoption; the snap moves it
    // from there.
    let relative = piece_world.affine().inverse() * socket_world.affine();
    Some((piece, Transform::from_matrix(relative.into())))
}

/// `socket.snap-*`: put every selected socket exactly on the nearest face,
/// edge or corner of its piece, aiming +Z out of it. One undoable transaction —
/// mating is unforgiving about both position and direction, and neither is
/// something you can eyeball.
fn snap_selected_sockets(world: &mut World, feature: crate::sockets::SnapFeature) {
    // The parent is OPTIONAL in the query on purpose: a socket sitting at the
    // scene root matched nothing at all when it was required, so the verb
    // reported "select a socket" at a socket that was plainly selected.
    let sockets: Vec<(SceneId, Transform, Option<Entity>)> = {
        let mut query = world.query_filtered::<
            (&SceneId, &Transform, Option<&ChildOf>),
            (With<crate::sockets::Socket>, With<Selected>),
        >();
        query
            .iter(world)
            .map(|(id, transform, child_of)| (*id, *transform, child_of.map(|c| c.parent())))
            .collect()
    };
    if sockets.is_empty() {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select a socket to snap".into(),
            success: false,
        });
        return;
    }
    let mut parentless = 0usize;
    let mut ops = Vec::new();
    for (socket_id, local, parent) in sockets {
        // A socket snaps to the PIECE it belongs to. With no parent, snap to
        // the nearest piece and ADOPT it — "put this socket on that object" is
        // the whole intent, and requiring a reparent first made the verb look
        // broken to anyone who just inserted a socket.
        let (parent, local) = match parent {
            Some(parent) => (parent, local),
            None => {
                let Some((piece, in_piece)) = nearest_piece(world, socket_id) else {
                    parentless += 1;
                    continue;
                };
                ops.push(Op::Reparent {
                    target: socket_id,
                    parent: world.get::<SceneId>(piece).copied(),
                });
                (piece, in_piece)
            }
        };
        let Some((min, max)) = piece_bounds(world, parent) else {
            continue; // nothing with bounds yet (model still loading)
        };
        let placed = crate::sockets::snap_to_bounds(local.translation, min, max, feature);
        ops.push(Op::Set {
            target: socket_id,
            value: Box::new(placed.with_scale(local.scale)).into_partial_reflect(),
        });
    }
    let snapped = ops.len();
    if snapped == 0 {
        world.write_message(editor_scene::SceneIoFeedback {
            message: if parentless > 0 {
                "a socket snaps to the piece it belongs to — parent it to one first".into()
            } else {
                "the piece has no geometry to snap to yet".to_string()
            },
            success: false,
        });
        return;
    }
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: "Snap Socket".into(),
        gesture: None,
        ops,
    });
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!(
            "snapped {snapped} socket{} to the nearest {}",
            if snapped == 1 { "" } else { "s" },
            match feature {
                crate::sockets::SnapFeature::Face => "face",
                crate::sockets::SnapFeature::Edge => "edge",
                crate::sockets::SnapFeature::Corner => "corner",
            }
        ),
        success: true,
    });
}

fn snap_moved_instance(world: &mut World, root_id: SceneId) {
    snap_instance_to_socket(world, root_id, None);
}

/// Mate `root_id` with the nearest compatible socket in reach. `gesture` tags
/// the transaction so a live drag coalesces into the SAME undo entry as the
/// move itself — snapping mid-drag must not cost a second undo.
fn snap_instance_to_socket(world: &mut World, root_id: SceneId, gesture: Option<u64>) {
    let Some(root) = world.resource::<SceneIndex>().get(&root_id) else {
        return;
    };
    // ANYTHING that owns sockets can mate. This used to require a
    // `PrefabInstance` and return in silence otherwise, so a socketed imported
    // model — the normal M4 asset — could never snap to anything, and nothing
    // said why. Socket generation never had that requirement, so the editor
    // happily let you author sockets onto a piece that could not use them.
    let own: Vec<Entity> = crate::open_mode::members_of(world, root)
        .into_iter()
        .filter(|entity| world.get::<crate::sockets::Socket>(*entity).is_some())
        .collect();
    // The sockets this piece ACTUALLY has, in root-relative frames. A template
    // is only the fallback for an instance whose sockets are not stamped yet.
    // Socket frames come from the propagated globals, where the lag cancels
    // out: the piece moves rigidly, so socket-relative-to-root is the same
    // whichever frame both were measured in.
    let propagated = world
        .get::<GlobalTransform>(root)
        .copied()
        .unwrap_or_default();
    let to_root = propagated.affine().inverse();
    // WHERE IT IS NOW comes from the Transform. `GlobalTransform` propagates in
    // `PostUpdate`, so mid-drag it still describes last frame's pose — measuring
    // the reach from that means measuring from where the piece USED to be.
    let root_global = world
        .get::<Transform>(root)
        .map(|transform| GlobalTransform::from(*transform))
        .unwrap_or(propagated);
    let mut mating: Vec<(Transform, crate::sockets::Socket)> = own
        .iter()
        .filter_map(|entity| {
            let global = world.get::<GlobalTransform>(*entity).copied()?;
            let socket = world.get::<crate::sockets::Socket>(*entity).cloned()?;
            let local = Transform::from_matrix((to_root * global.affine()).into());
            Some((local, socket))
        })
        .collect();
    if mating.is_empty()
        && let Some(instance) = world.get::<PrefabInstance>(root).copied()
        && let Some(def) = world.resource::<PrefabLibrary>().prefabs.get(&instance.0)
    {
        mating = crate::sockets::template_sockets(def);
    }
    if mating.is_empty() {
        return; // nothing to mate WITH; not a failure, just not a socketed piece
    }
    // Own sockets are excluded by FILTERING, not by tearing the component off
    // every socket and putting it back: that fired the add-observer again on
    // every pass, and the gizmo it spawns is a child — a drag left a heap of
    // stacked cones behind, which is most of what "I can hardly use the
    // sockets" looks like on screen.
    let radius = world.resource::<EditorSettings>().viewport.socket_reach;
    let candidates = crate::sockets::mate_candidates(world, &own);
    let Some(found) = crate::sockets::best_mate(&candidates, &mating, &root_global, radius) else {
        // Say so. A silent refusal is indistinguishable from a broken editor,
        // and this one was silent for every reason it could have.
        if gesture.is_none() {
            world.write_message(editor_scene::SceneIoFeedback {
                message: format!("no compatible socket within {radius:.1}m"),
                success: false,
            });
        }
        return;
    };
    world.resource_mut::<EditQueue>().0.push(Transaction {
        label: "Snap To Socket".into(),
        gesture,
        ops: vec![Op::Set {
            target: root_id,
            value: Box::new(found.root).into_partial_reflect(),
        }],
    });
    // A live drag would repeat this every frame — say it once, on commit.
    if gesture.is_none() {
        world.write_message(editor_scene::SceneIoFeedback {
            message: found.label,
            success: true,
        });
    }
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

/// LIVE socket snapping (owner ask): while you drag a single prefab instance,
/// it mates with a compatible socket the moment one is in reach — so rotating a
/// wall 90° and dragging it against another forms the corner in front of you,
/// instead of after you let go and hope.
///
/// The snap rides the gesture's own id, so the whole drag stays ONE undo entry;
/// and it re-derives from the gesture's originals every frame, so pulling out of
/// range simply hands control back to the cursor.
pub(crate) fn snap_during_drag(world: &mut World) {
    let dragging = match &*world.resource::<MoveGesture>() {
        MoveGesture::Active {
            id,
            kind: editor_core::gesture::GestureKind::Move,
            originals,
            ..
        } if originals.len() == 1 => Some((*id, originals[0].0)),
        // Rotation is aiming, not placing — snapping mid-rotate would fight the
        // angle you are dialling in.
        _ => None,
    };
    let Some((gesture, root_id)) = dragging else {
        return;
    };
    let is_instance = world
        .resource::<SceneIndex>()
        .get(&root_id)
        .is_some_and(|root| world.get::<PrefabInstance>(root).is_some());
    if !is_instance {
        return;
    }
    snap_instance_to_socket(world, root_id, Some(gesture));
}

/// PIVOT ON THE JOINT (owner: "pivot on socket needs to pivot on connected
/// sockets").
///
/// Rotating a piece that is attached to something should swing it about the
/// place it is ATTACHED — the joint stays put and the far end sweeps, which is
/// how a corner gets made and how a curve gets walked round. Pivoting about the
/// piece's own centre tears the joint apart on the first degree.
///
/// Two ways to say which point:
/// - select a SOCKET, and that is the pivot — the explicit answer, for a piece
///   with several joints or none;
/// - select the PIECE, and its connected socket is found for you. That is the
///   common case and it needed no words.
///
/// Connection is derived from geometry rather than stored, so it is equally
/// true for a joint a designer made by hand. Ties break on the socket nearest
/// the piece's origin, so a repeated rotate keeps using the same joint instead
/// of alternating between two.
///
/// Kept up to date every frame rather than at action time: the gesture reads it
/// the instant `r` arrives, and a stale pin would rotate the wrong thing about
/// the wrong point.
pub(crate) fn pin_pivot_to_selected_socket(
    selected_sockets: Query<
        (Entity, &GlobalTransform),
        (With<crate::sockets::Socket>, With<Selected>),
    >,
    all_sockets: Query<(Entity, &GlobalTransform, &crate::sockets::Socket)>,
    selected_roots: Query<(Entity, &SceneId, &GlobalTransform), With<Selected>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    instances: Query<&SceneId, With<PrefabInstance>>,
    stamped: Query<&StampedFrom>,
    mut pin: ResMut<editor_core::gesture::GesturePivot>,
) {
    let owner_of = |socket: Entity| -> Option<SceneId> {
        let mut current = socket;
        loop {
            if let Ok(id) = instances.get(current) {
                return Some(*id);
            }
            if let Ok(from) = stamped.get(current) {
                return Some(from.instance_root);
            }
            current = parents.get(current).ok()?.parent();
        }
    };

    // An explicitly selected socket wins: the designer named the point.
    let mut found = selected_sockets
        .iter()
        .next()
        .and_then(|(socket, global)| Some((owner_of(socket)?, global.translation())));

    // Otherwise: the selected PIECE's joint, if it has one.
    if found.is_none()
        && let Some((root, root_id, root_global)) = selected_roots.iter().next()
    {
        let mut mine: Vec<(Entity, GlobalTransform, crate::sockets::Socket)> = Vec::new();
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if let Ok((_, global, socket)) = all_sockets.get(entity) {
                mine.push((entity, *global, socket.clone()));
            }
            if let Ok(kids) = children.get(entity) {
                stack.extend(kids.iter());
            }
        }
        let mut best: Option<(f32, Vec3)> = None;
        for (entity, global, socket) in &mine {
            for (other, other_global, other_socket) in all_sockets.iter() {
                if other == *entity || socket.socket_type != other_socket.socket_type {
                    continue;
                }
                // A socket of MY OWN piece is not something I am joined to.
                if mine.iter().any(|(mine_entity, _, _)| *mine_entity == other) {
                    continue;
                }
                if !crate::sockets::sockets_are_joined(global, other_global) {
                    continue;
                }
                let reach = global.translation().distance(root_global.translation());
                if best
                    .as_ref()
                    .is_none_or(|(best_reach, _)| reach < *best_reach)
                {
                    best = Some((reach, global.translation()));
                }
            }
        }
        if let Some((_, at)) = best {
            found = Some((*root_id, at));
        }
    }

    let (subject, pivot) = match found {
        Some((owner, at)) => (Some(owner), Some(at)),
        None => (None, None),
    };
    if pin.subject != subject || pin.pivot != pivot {
        pin.subject = subject;
        pin.pivot = pivot;
    }
}
