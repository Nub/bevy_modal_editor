//! Prefab authoring verbs (M4-D5 close): create-from-selection, revert
//! overrides, apply-to-prefab, library dir loading, generation-driven restamp.

use crate::{
    stamp_prefab, OverridePatch, PrefabDef, PrefabInstance, PrefabLibrary, PrefabOverrides,
    Stamped, StampedFrom,
};
use bevy::prelude::*;
use editor_core::edits::EditorComponents;
use editor_core::prelude::*;
use editor_scene::{snapshot_from_parts, PrefabStamped};
use std::path::PathBuf;
use uuid::Uuid;

pub const PREFABS_DIR: &str = "prefabs";

#[derive(Resource, Default)]
pub(crate) struct PrefabRequests {
    create: bool,
    revert: bool,
    apply: bool,
    edit: bool,
}

pub(crate) fn collect_prefab_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<EditorState>,
    mut requests: ResMut<PrefabRequests>,
) {
    if !state.active {
        return;
    }
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "prefab.create" => requests.create = true,
            "prefab.revert-overrides" => requests.revert = true,
            "prefab.apply-to-prefab" => requests.apply = true,
            "prefab.edit" => requests.edit = true,
            _ => {}
        }
    }
}

/// Startup: load every prefabs/*.prefab.ron into the library.
pub(crate) fn load_prefab_library(world: &mut World) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let Ok(entries) = std::fs::read_dir(PREFABS_DIR) else { return };
    let mut loaded = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.to_string_lossy().ends_with(".prefab.ron") {
            continue;
        }
        match PrefabDef::load(&path, &registry) {
            Ok(def) => {
                world.resource_mut::<PrefabLibrary>().prefabs.insert(def.id, def);
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

pub(crate) fn save_prefab_public(world: &World, def: &PrefabDef) {
    save_prefab(world, def)
}

fn save_prefab(world: &World, def: &PrefabDef) {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let _ = std::fs::create_dir_all(PREFABS_DIR);
    let path = PathBuf::from(PREFABS_DIR)
        .join(format!("{}.prefab.ron", def.name.to_lowercase().replace(' ', "-")));
    if let Err(e) = def.save(&path, &registry.read()) {
        error!("prefab save failed: {e}");
    }
}

/// The verbs (exclusive; scene mutations go through the EditQueue).
pub(crate) fn perform_prefab_actions(world: &mut World) {
    let requests = std::mem::take(&mut *world.resource_mut::<PrefabRequests>());

    if requests.edit {
        crate::edit_mode::toggle_edit_mode(world);
    }
    if requests.create {
        create_from_selection(world);
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
        world.entity_mut(entity).despawn();
    }
    let Some(root) = world.resource::<SceneIndex>().get(&root_id) else { return };
    let Some(instance) = world.get::<PrefabInstance>(root).copied() else { return };
    stamp_prefab(world, instance.0, root);
}

/// Revert: clear the deltas, restamp clean. (Undo nuance documented: the
/// override component Set is undoable, but the diff re-derives from restamped
/// state — revert is treated as a deliberate reset, not a history entry.)
fn revert_overrides(world: &mut World, root_id: SceneId) {
    let Some(root) = world.resource::<SceneIndex>().get(&root_id) else { return };
    if let Some(mut overrides) = world.get_mut::<PrefabOverrides>(root) {
        overrides.0.clear();
    }
    restamp(world, root_id);
}

/// Fold this instance's deltas INTO the template, save, propagate everywhere.
fn apply_to_prefab(world: &mut World, root_id: SceneId) {
    let Some(root) = world.resource::<SceneIndex>().get(&root_id) else { return };
    let Some(instance) = world.get::<PrefabInstance>(root).copied() else { return };
    let patches: Vec<OverridePatch> =
        world.get::<PrefabOverrides>(root).map(|o| o.0.clone()).unwrap_or_default();
    if patches.is_empty() {
        return;
    }
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    {
        let mut library = world.resource_mut::<PrefabLibrary>();
        let Some(prefab) = library.prefabs.get_mut(&instance.0) else { return };
        // Rebuild the template with patches folded in.
        let records: Vec<(SceneId, Option<SceneId>, Vec<Box<dyn bevy::reflect::PartialReflect>>)> =
            prefab
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
                            for patch in patches.iter().filter(|p| {
                                p.entity == id.0.to_string() && p.type_path == type_path
                            }) {
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

/// Create a prefab from the selection: selected non-stamped entities become the
/// template (their SceneIds become template-local ids); saved to prefabs/.
fn create_from_selection(world: &mut World) {
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    let components = world.resource::<EditorComponents>().types.clone();
    let selected: Vec<(Entity, SceneId)> = {
        let mut query = world
            .query_filtered::<(Entity, &SceneId), (With<Selected>, Without<PrefabStamped>)>();
        let mut all: Vec<_> = query.iter(world).map(|(e, id)| (e, *id)).collect();
        all.sort_by_key(|(_, id)| id.0);
        all
    };
    if selected.is_empty() {
        return;
    }
    let name = world
        .get::<Name>(selected[0].0)
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| "Prefab".to_string());
    let records: Vec<(SceneId, Option<SceneId>, Vec<Box<dyn bevy::reflect::PartialReflect>>)> =
        selected
            .iter()
            .map(|(entity, id)| {
                let values = components
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
                    .and_then(|c| world.get::<SceneId>(c.parent()))
                    .copied()
                    .filter(|p| selected.iter().any(|(_, id)| id == p));
                (*id, parent, values)
            })
            .collect();
    drop(registry);
    let def = PrefabDef { id: Uuid::new_v4(), name, template: snapshot_from_parts(records) };
    save_prefab(world, &def);
    let count = def.template.records().count();
    info!("created prefab '{}' ({count} entities)", def.name);
    world.resource_mut::<PrefabLibrary>().prefabs.insert(def.id, def);
    world.resource_mut::<PrefabLibrary>().generation += 1;
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
