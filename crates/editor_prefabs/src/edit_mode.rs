//! Prefab edit mode (M4-D7, pulled forward by owner): `space e` on a selected
//! instance swaps the WORLD for the prefab's template — real scene entities,
//! every editor tool works verbatim (hierarchy, reparent, insert, delete,
//! materials) — with its OWN undo scope. `space e` again captures the edited
//! world as the new template, saves the asset, restores the original scene,
//! and bumps the library generation so every instance restamps.

use crate::{PrefabInstance, PrefabLibrary, StampedFrom};
use bevy::prelude::*;
use editor_core::prelude::*;
use editor_scene::{apply_scene, capture_scene, snapshot_from_parts, SceneIoLock, SceneSnapshot};
use uuid::Uuid;

pub struct EditingPrefab {
    pub prefab: Uuid,
    saved_scene: SceneSnapshot,
    saved_selection: Vec<SceneId>,
}

#[derive(Resource, Default)]
pub struct PrefabEditState(pub Option<EditingPrefab>);

pub(crate) fn toggle_edit_mode(world: &mut World) {
    let editing = world.resource::<PrefabEditState>().0.is_some();
    if editing {
        finish(world);
    } else {
        enter(world);
    }
}

fn enter(world: &mut World) {
    // Resolve the prefab from the selection (instance root, or stamped child's root).
    let prefab: Option<Uuid> = {
        let mut roots: Vec<SceneId> = Vec::new();
        let mut direct: Option<Uuid> = None;
        let mut query = world.query_filtered::<(
            Option<&PrefabInstance>,
            Option<&StampedFrom>,
        ), With<Selected>>();
        for (instance, stamped) in query.iter(world) {
            if let Some(instance) = instance {
                direct = Some(instance.0);
            } else if let Some(stamped) = stamped {
                roots.push(stamped.instance_root);
            }
        }
        direct.or_else(|| {
            roots.first().and_then(|root_id| {
                world
                    .resource::<SceneIndex>()
                    .get(root_id)
                    .and_then(|e| world.get::<PrefabInstance>(e))
                    .map(|i| i.0)
            })
        })
    };
    let Some(prefab) = prefab else {
        world.write_message(editor_scene::SceneIoFeedback {
            message: "select a prefab instance to edit".into(),
            success: false,
        });
        return;
    };
    let saved_scene = capture_scene(world);
    let saved_selection: Vec<SceneId> = {
        let mut query = world.query_filtered::<&SceneId, With<Selected>>();
        query.iter(world).copied().collect()
    };
    // Template -> REAL scene entities (template ids as SceneIds, fully editable),
    // own undo scope (clear_history).
    let template = {
        let library = world.resource::<PrefabLibrary>();
        let Some(def) = library.prefabs.get(&prefab) else { return };
        snapshot_from_parts(
            def.template
                .records()
                .map(|(id, parent, c)| (id, parent, c.iter().map(|v| v.to_dynamic()).collect()))
                .collect(),
        )
    };
    apply_scene(world, &template, true);
    world.resource_mut::<SceneIoLock>().0 = true;
    world.resource_mut::<PrefabEditState>().0 =
        Some(EditingPrefab { prefab, saved_scene, saved_selection });
    let name = world.resource::<PrefabLibrary>().prefabs[&prefab].name.clone();
    world.write_message(editor_scene::SceneIoFeedback {
        message: format!("editing prefab '{name}' — space e to finish"),
        success: true,
    });
}

fn finish(world: &mut World) {
    let Some(editing) = world.resource_mut::<PrefabEditState>().0.take() else { return };
    // The edited world IS the new template.
    let new_template = capture_scene(world);
    {
        let mut library = world.resource_mut::<PrefabLibrary>();
        if let Some(def) = library.prefabs.get_mut(&editing.prefab) {
            def.template = new_template;
        }
    }
    // Save the asset.
    let def_clone = {
        let library = world.resource::<PrefabLibrary>();
        library.prefabs.get(&editing.prefab).map(|p| crate::PrefabDef {
            id: p.id,
            name: p.name.clone(),
            template: snapshot_from_parts(
                p.template
                    .records()
                    .map(|(id, parent, c)| {
                        (id, parent, c.iter().map(|v| v.to_dynamic()).collect())
                    })
                    .collect(),
            ),
        })
    };
    if let Some(def) = def_clone {
        crate::authoring::save_prefab_public(world, &def);
    }
    // Restore the original scene + selection; propagate via generation bump.
    apply_scene(world, &editing.saved_scene, true);
    for id in &editing.saved_selection {
        if let Some(entity) = world.resource::<SceneIndex>().get(id) {
            world.entity_mut(entity).insert(Selected);
        }
    }
    world.resource_mut::<SceneIoLock>().0 = false;
    world.resource_mut::<PrefabLibrary>().generation += 1;
    world.write_message(editor_scene::SceneIoFeedback {
        message: "prefab saved — instances updated".into(),
        success: true,
    });
}
