//! Material editor probe (MATERIAL_PROBE=1, M4-D11): drive the real flow —
//! create a material, open the editor, edit through the actual widget events
//! (drag coalescing included), verify the ASSET-scoped undo never touches the
//! scene history, confirm persistence, close by the escape grammar. Cleans up
//! its own material so the library never accumulates probe artifacts.

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use bevy::ui_widgets::ValueChange;
use editor_core::prelude::*;
use editor_scene::materials::{MaterialLibrary, load_materials};
use uuid::Uuid;

use crate::material_editor::{Field, MaterialEditorRoot, MaterialEditorState, MaterialPreviewRig};
use crate::probe_user::{shot, tap, tap_named};

#[derive(Resource, Default)]
pub(crate) struct MaterialProbe {
    frame: u32,
    failures: Vec<String>,
    created: Option<Uuid>,
    scene_undo_before: usize,
    /// The model placed to prove assignment reaches derived gltf geometry.
    placed: Option<SceneId>,
}

/// Every material handle on a mesh under `root`, derived subtree included —
/// what the user actually SEES shading a placed model.
fn subtree_materials(world: &mut World, root: Entity) -> Vec<Option<Handle<StandardMaterial>>> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if world.get::<Mesh3d>(entity).is_some() {
            found.push(
                world
                    .get::<MeshMaterial3d<StandardMaterial>>(entity)
                    .map(|m| m.0.clone()),
            );
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    found
}

fn check(world: &mut World, ok: bool, what: &str) {
    if ok {
        info!("MATERIAL-PROBE PASS: {what}");
    } else {
        error!("MATERIAL-PROBE FAIL: {what}");
        world
            .resource_mut::<MaterialProbe>()
            .failures
            .push(what.to_string());
    }
}

fn invoke(world: &mut World, action: &'static str) {
    world.write_message(ActionInvoked {
        action: ActionId::new_static(action),
        args: None,
        source: InvocationSource::Test,
    });
}

fn created_def(world: &mut World) -> Option<editor_scene::materials::MaterialDef> {
    let id = world.resource::<MaterialProbe>().created?;
    world.resource::<MaterialLibrary>().get(&id).cloned()
}

/// Fire the SAME entity event the widget produces — the observer path under
/// test is identical to a real drag.
fn slider_change(world: &mut World, field: Field, value: f32, is_final: bool) {
    let slider = world
        .query::<(Entity, &Field)>()
        .iter(world)
        .find(|(_, f)| **f == field)
        .map(|(e, _)| e);
    match slider {
        Some(source) => world.trigger(ValueChange {
            source,
            value,
            is_final,
        }),
        None => check(world, false, &format!("widget for {field:?} exists")),
    }
}

pub(crate) fn probe_material(world: &mut World) {
    world.resource_mut::<MaterialProbe>().frame += 1;
    let frame = world.resource::<MaterialProbe>().frame;
    if frame == 1 {
        let _ = std::fs::create_dir_all(crate::probe_user::SHOT_DIR);
        info!("MATERIAL-PROBE armed");
    }
    match frame {
        60 => tap_named(world, KeyCode::Enter, Key::Enter),
        120 => invoke(world, "core.toggle-editor"),
        // ── Create + open ──────────────────────────────────────────────────
        160 => {
            let before = world.resource::<MaterialLibrary>().materials.len();
            world.resource_mut::<MaterialProbe>().scene_undo_before = before; // reuse slot briefly
            invoke(world, "material.new");
        }
        200 => {
            let (grew, newest) = {
                let library = world.resource::<MaterialLibrary>();
                let before = world.resource::<MaterialProbe>().scene_undo_before;
                (
                    library.materials.len() == before + 1,
                    library.materials.last().map(|def| def.id),
                )
            };
            check(world, grew, "material.new added a library material");
            world.resource_mut::<MaterialProbe>().created = newest;
        }
        220 => invoke(world, "material.edit"),
        260 => {
            let state_ok = {
                let state = world.resource::<MaterialEditorState>();
                let created = world.resource::<MaterialProbe>().created;
                state.open && state.target == created
            };
            check(world, state_ok, "material.edit opened on the new material");
            let visible = world
                .query_filtered::<&Visibility, With<MaterialEditorRoot>>()
                .iter(world)
                .any(|v| *v == Visibility::Visible);
            check(world, visible, "the editor surface is visible");
            let camera_on = {
                let camera = world.resource::<MaterialPreviewRig>().camera;
                world.get::<Camera>(camera).is_some_and(|c| c.is_active)
            };
            check(world, camera_on, "the preview camera renders while open");
            let scoped = *world.resource::<HistoryScope>() == HistoryScope::Asset;
            check(
                world,
                scoped,
                "the open editor claims the asset history scope",
            );
        }
        // ── Edit through the real widget events (drag then release) ───────
        300 => {
            world.resource_mut::<MaterialProbe>().scene_undo_before =
                world.resource::<History>().undo_depth();
            slider_change(world, Field::Metallic, 0.5, false);
        }
        // MID-drag (is_final was false): the thumb must already have moved.
        // Writing `SliderValue` back is the app's job — skip it and the slider
        // takes the pointer but never visibly slides.
        305 => {
            let shown = world
                .query::<(&Field, &bevy::ui_widgets::SliderValue)>()
                .iter(world)
                .find(|(f, _)| matches!(f, Field::Metallic))
                .map(|(_, v)| v.0);
            check(
                world,
                shown.is_some_and(|v| (v - 0.5).abs() < 1e-5),
                "the thumb follows the pointer DURING a drag",
            );
        }
        310 => slider_change(world, Field::Metallic, 0.9, true),
        340 => {
            let metallic_ok = created_def(world).is_some_and(|d| (d.metallic - 0.9).abs() < 1e-5);
            check(world, metallic_ok, "slider drag committed metallic 0.9");
            let preview_ok = {
                let handle = world.resource::<MaterialPreviewRig>().material.clone();
                world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&handle)
                    .is_some_and(|m| (m.metallic - 0.9).abs() < 1e-5)
            };
            check(
                world,
                preview_ok,
                "the preview material tracks the edit live",
            );
        }
        360 => slider_change(world, Field::Roughness, 0.2, true),
        // ── Asset-scoped undo: material unwinds, scene history untouched ───
        400 => invoke(world, "core.undo"),
        440 => {
            let def = created_def(world);
            check(
                world,
                def.as_ref()
                    .is_some_and(|d| (d.roughness - 0.6).abs() < 1e-5),
                "undo reverted the roughness edit",
            );
            check(
                world,
                def.is_some_and(|d| (d.metallic - 0.9).abs() < 1e-5),
                "undo peeled ONE entry — the coalesced metallic drag survives",
            );
        }
        460 => invoke(world, "core.undo"),
        480 => {
            let metallic_zero = created_def(world).is_some_and(|d| d.metallic.abs() < 1e-5);
            check(world, metallic_zero, "second undo reverted the whole drag");
            let scene_ok = world.resource::<History>().undo_depth()
                == world.resource::<MaterialProbe>().scene_undo_before;
            check(world, scene_ok, "scene history untouched by asset undo");
        }
        500 => invoke(world, "core.redo"),
        540 => {
            let metallic_back = created_def(world).is_some_and(|d| (d.metallic - 0.9).abs() < 1e-5);
            check(world, metallic_back, "redo restored the drag");
            slider_change(world, Field::Roughness, 0.2, true);
        }
        // ── Persistence: the library saved the edited def to disk ──────────
        620 => {
            let on_disk = {
                let (path, id) = {
                    let library = world.resource::<MaterialLibrary>();
                    (
                        library.path.clone(),
                        world.resource::<MaterialProbe>().created,
                    )
                };
                load_materials(&path)
                    .ok()
                    .zip(id)
                    .is_some_and(|(materials, id)| {
                        materials
                            .iter()
                            .any(|d| d.id == id && (d.metallic - 0.9).abs() < 1e-5)
                    })
            };
            check(world, on_disk, "edits persisted to materials.ron");
            let metallic_widget = world
                .query::<(&Field, &bevy::ui_widgets::SliderValue)>()
                .iter(world)
                .find(|(f, _)| matches!(f, Field::Metallic))
                .map(|(_, v)| v.0);
            check(
                world,
                metallic_widget.is_some_and(|v| (v - 0.9).abs() < 1e-5),
                "the widget DISPLAYS the committed value",
            );
            // The red track must paint THIS material's red axis. Left unset,
            // `SliderBaseColor` is white — which renders red as cyan→white,
            // a CMYK-looking track under an RGB label.
            let base_tracked = {
                let def = created_def(world);
                let track = world
                    .query::<(&Field, &bevy::feathers::controls::SliderBaseColor)>()
                    .iter(world)
                    .find(|(f, _)| matches!(f, Field::BaseR))
                    .map(|(_, b)| b.0.to_srgba());
                def.zip(track).is_some_and(|(def, track)| {
                    (track.red - def.base_color[0]).abs() < 1e-3
                        && (track.green - def.base_color[1]).abs() < 1e-3
                        && (track.blue - def.base_color[2]).abs() < 1e-3
                })
            };
            check(
                world,
                base_tracked,
                "color tracks paint the material's own axes, not white's",
            );
            shot(world, "21-material-editor");
        }
        // ── Rename through THE name prompt, undoable like any material edit ─
        640 => invoke(world, "material.rename"),
        660 => {
            let open = world
                .resource::<editor_prefabs::authoring::GroupPrompt>()
                .open;
            check(world, open, "material.rename opens the name prompt");
        }
        664 => tap(world, KeyCode::KeyT, "t"),
        668 => tap(world, KeyCode::KeyI, "i"),
        672 => tap(world, KeyCode::KeyN, "n"),
        676 => tap_named(world, KeyCode::Enter, Key::Enter),
        690 => {
            let renamed = created_def(world).is_some_and(|d| d.name == "tin");
            check(world, renamed, "the committed name reached the library");
            invoke(world, "core.undo");
        }
        694 => {
            let reverted = created_def(world).is_some_and(|d| d.name != "tin");
            check(
                world,
                reverted,
                "undo takes the rename back like any other material edit",
            );
        }
        696 => invoke(world, "core.redo"),
        // ── Escape grammar: empty-handed Esc closes; scope returns ─────────
        700 => tap_named(world, KeyCode::Escape, Key::Escape),
        740 => {
            let closed = !world.resource::<MaterialEditorState>().open;
            check(world, closed, "empty-handed Escape closed the editor");
            let scope_back = *world.resource::<HistoryScope>() == HistoryScope::Scene;
            check(world, scope_back, "closing returned Ctrl+Z to the scene");
        }
        // ── Assignment reaches a MODEL's geometry (the derived gltf subtree,
        //    which is where the meshes actually are — spec §6/§7) ───────────
        760 => {
            let model = world
                .resource::<editor_scene::models::ModelLibrary>()
                .entries
                .iter()
                .find(|entry| entry.kind == editor_scene::models::EntryKind::Model)
                .map(|entry| entry.uuid);
            match model {
                Some(model) => {
                    let id = SceneId::random();
                    world.resource_mut::<MaterialProbe>().placed = Some(id);
                    world.resource_mut::<EditQueue>().0.push(Transaction {
                        label: "Place Model".into(),
                        gesture: None,
                        ops: vec![Op::Spawn {
                            id,
                            components: vec![
                                Box::new(editor_scene::models::MeshRef(model))
                                    .into_partial_reflect(),
                                Box::new(Transform::from_translation(Vec3::new(0.0, 0.0, -6.0)))
                                    .into_partial_reflect(),
                                Box::new(Name::new("probe model")).into_partial_reflect(),
                            ],
                        }],
                    });
                }
                None => info!("MATERIAL-PROBE SKIP: no imported model to shade"),
            }
        }
        // The gltf subtree resolves asynchronously — assign while it is still
        // loading, exactly as a user would after placing.
        790 => {
            if let (Some(id), Some(material)) = (
                world.resource::<MaterialProbe>().placed,
                world.resource::<MaterialProbe>().created,
            ) {
                world.resource_mut::<EditQueue>().0.push(Transaction {
                    label: "Assign Material".into(),
                    gesture: None,
                    ops: vec![Op::Set {
                        target: id,
                        value: Box::new(editor_scene::materials::MaterialRef(material))
                            .into_partial_reflect(),
                    }],
                });
            }
        }
        900 => {
            if let (Some(id), Some(material)) = (
                world.resource::<MaterialProbe>().placed,
                world.resource::<MaterialProbe>().created,
            ) {
                let assigned = world
                    .resource::<editor_scene::materials::MaterialHandles>()
                    .0
                    .get(&material)
                    .cloned();
                let root = world.resource::<editor_api::edits::SceneIndex>().get(&id);
                let meshes = root
                    .map(|root| subtree_materials(world, root))
                    .unwrap_or_default();
                check(
                    world,
                    !meshes.is_empty(),
                    "the placed model spawned its gltf meshes",
                );
                check(
                    world,
                    assigned.is_some()
                        && meshes
                            .iter()
                            .all(|handle| handle.as_ref() == assigned.as_ref()),
                    "the assigned material reached EVERY mesh in the model subtree",
                );
                shot(world, "23-model-material");
            }
        }
        // ── Cleanup: the probe's artifacts never outlive the run ───────────
        940 => {
            if let Some(id) = world.resource::<MaterialProbe>().placed
                && let Some(entity) = world.resource::<editor_api::edits::SceneIndex>().get(&id)
            {
                world.entity_mut(entity).despawn();
            }
        }
        960 => {
            if let Some(id) = world.resource::<MaterialProbe>().created {
                let mut library = world.resource_mut::<MaterialLibrary>();
                library.materials.retain(|d| d.id != id);
                library.generation += 1;
            }
        }
        1000 => {
            let failures = world.resource::<MaterialProbe>().failures.clone();
            if failures.is_empty() {
                info!("MATERIAL-PROBE PASS: the material editor end-to-end");
                world.write_message(AppExit::Success);
            } else {
                error!("MATERIAL-PROBE FAILED: {failures:?}");
                world.write_message(AppExit::error());
            }
        }
        _ => {}
    }
}
