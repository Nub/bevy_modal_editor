//! Material editor probe (MATERIAL_PROBE=1, M4-D11): drive the real flow —
//! create a material, open the editor, edit through the actual widget events
//! (drag coalescing included), verify the ASSET-scoped undo never touches the
//! scene history, confirm persistence, close by the escape grammar. Cleans up
//! its own material so the library never accumulates probe artifacts.

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use bevy::ui_widgets::ValueChange;
use editor_core::prelude::*;
use editor_scene::materials::{MaterialLibrary, TextureSlot, load_materials};
use uuid::Uuid;

use crate::material_editor::{Field, MaterialEditorRoot, MaterialEditorState, MaterialPreviewRig};
use crate::probe_user::{click, move_cursor, shot, tap, tap_named};

#[derive(Resource, Default)]
pub(crate) struct MaterialProbe {
    library_before: usize,
    target_before: Option<Uuid>,
    before_detach: Option<editor_scene::materials::MaterialDef>,
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
        // ── Texture slots and tiling (format 2) ───────────────────────────
        // The panel builds one row per DECLARED slot, so every slot must have a
        // live chip — a table with no way to fill it is not authoring.
        550 => {
            let present: Vec<TextureSlot> = TextureSlot::ALL
                .into_iter()
                .filter(|slot| {
                    world
                        .query::<&Field>()
                        .iter(world)
                        .any(|field| *field == Field::Texture(*slot))
                })
                .collect();
            check(
                world,
                present.len() == TextureSlot::ALL.len(),
                &format!("every texture slot has a row ({present:?})"),
            );
            // A data map must not be gamma-decoded on load; the slot decides
            // it, so the contract cannot be forgotten at a call site.
            let linear = TextureSlot::ALL
                .into_iter()
                .filter(|slot| !slot.is_srgb())
                .count();
            check(
                world,
                linear == 3,
                "normal, metal/rough and occlusion load linear, not sRGB",
            );
        }
        560 => slider_change(world, Field::UvTilingX, 4.0, true),
        570 => slider_change(world, Field::UvTilingY, 2.0, true),
        600 => {
            let tiling = created_def(world).map(|def| def.uv_tiling);
            check(
                world,
                tiling.is_some_and(|t| (t[0] - 4.0).abs() < 0.01 && (t[1] - 2.0).abs() < 0.01),
                &format!("the tiling sliders write into the def ({tiling:?})"),
            );
            let handle = world.resource::<MaterialPreviewRig>().material.clone();
            let scale = world
                .resource::<Assets<StandardMaterial>>()
                .get(&handle)
                .map(|m| m.uv_transform.matrix2.x_axis.x);
            check(
                world,
                scale.is_some_and(|x| (x - 4.0).abs() < 0.01),
                &format!("tiling reaches the rendered material ({scale:?})"),
            );
            shot(world, "60-material-tiling");
        }
        // The colour-space contract has to be checked where it MATTERS: on the
        // image the GPU samples. A CPU-side assertion on the def cannot fail
        // for the bug that mattered here — the asset server keys handles by
        // path, so an earlier default-settings load of the same file would win
        // and silently gamma-decode a data map while every def-level check
        // stayed green.
        610 => {
            let texture = world
                .resource::<editor_scene::models::ModelLibrary>()
                .entries
                .iter()
                .find(|entry| entry.kind == editor_scene::models::EntryKind::Texture)
                .map(|entry| entry.uuid);
            match texture {
                Some(uuid) => {
                    let id = world.resource::<MaterialProbe>().created;
                    if let Some(id) = id {
                        let mut library = world.resource_mut::<MaterialLibrary>();
                        if let Some(def) = library.get_mut(&id) {
                            def.set_texture(TextureSlot::Normal, Some(uuid));
                        }
                    }
                }
                None => check(world, false, "an imported texture exists to bind"),
            }
        }
        682 => {
            let handle = world.resource::<MaterialPreviewRig>().material.clone();
            let normal = world
                .resource::<Assets<StandardMaterial>>()
                .get(&handle)
                .and_then(|m| m.normal_map_texture.clone());
            check(
                world,
                normal.is_some(),
                "the normal slot reaches StandardMaterial::normal_map_texture",
            );
            if let Some(normal) = normal {
                let (srgb, repeats) = {
                    let images = world.resource::<Assets<Image>>();
                    let image = images.get(&normal);
                    (
                        image.map(|i| i.texture_descriptor.format.is_srgb()),
                        image.map(|i| match &i.sampler {
                            bevy::image::ImageSampler::Descriptor(descriptor) => {
                                descriptor.address_mode_u == bevy::image::ImageAddressMode::Repeat
                            }
                            bevy::image::ImageSampler::Default => false,
                        }),
                    )
                };
                check(
                    world,
                    srgb == Some(false),
                    &format!("the normal map is loaded LINEAR, not sRGB ({srgb:?})"),
                );
                check(
                    world,
                    repeats == Some(true),
                    &format!("the sampler REPEATS, so tiling tiles ({repeats:?})"),
                );
            }
        }
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
        // ── Library verbs: the library used to only grow ──────────────────
        1020 => {
            // Target explicitly rather than inheriting whatever the run left
            // open: these checks are about the verbs, not about panel state.
            let first = world
                .resource::<MaterialLibrary>()
                .materials
                .first()
                .map(|def| def.id);
            world.resource_mut::<MaterialEditorState>().target = first;
            let before = world.resource::<MaterialLibrary>().materials.len();
            world.resource_mut::<MaterialProbe>().library_before = before;
            world.resource_mut::<MaterialProbe>().target_before = first;
            invoke(world, "material.duplicate");
        }
        1060 => {
            let before = world.resource::<MaterialProbe>().library_before;
            let after = world.resource::<MaterialLibrary>().materials.len();
            check(
                world,
                after == before + 1,
                &format!("duplicate added a material ({before} then {after})"),
            );
            let copy = world
                .resource::<MaterialLibrary>()
                .materials
                .last()
                .cloned();
            check(
                world,
                copy.as_ref().is_some_and(|def| def.name.ends_with(" copy")),
                "the copy is named after its source",
            );
            // A duplicate is how a VARIANT starts, so the panel follows the copy.
            let target = world.resource::<MaterialEditorState>().target;
            let before_target = world.resource::<MaterialProbe>().target_before;
            check(
                world,
                target.is_some() && target != before_target,
                "the editor follows the copy, ready to tweak",
            );
        }
        // Delete refuses while anything still wears the material: there is no
        // asset-history entry to undo it with, and a shaded object would be
        // silently unpainted.
        1100 => {
            // Put the material ON something, so the refusal is about a real
            // reference rather than about an empty scene.
            let id = world
                .resource::<MaterialLibrary>()
                .materials
                .first()
                .map(|def| def.id);
            match id {
                Some(id) => {
                    world.spawn(editor_scene::materials::MaterialRef(id));
                    world.resource_mut::<MaterialEditorState>().target = Some(id);
                    let before = world.resource::<MaterialLibrary>().materials.len();
                    world.resource_mut::<MaterialProbe>().library_before = before;
                    invoke(world, "material.delete");
                }
                None => check(world, false, "a material exists to try deleting"),
            }
        }
        1140 => {
            let before = world.resource::<MaterialProbe>().library_before;
            let after = world.resource::<MaterialLibrary>().materials.len();
            check(
                world,
                after == before,
                &format!("delete REFUSED while the material is in use ({after} of {before})"),
            );
        }
        // The copy nothing wears can go — which is the whole point: a
        // mis-created material used to be permanent.
        1180 => {
            let unused = {
                let used: Vec<Uuid> = world
                    .query::<&editor_scene::materials::MaterialRef>()
                    .iter(world)
                    .map(|reference| reference.0)
                    .collect();
                world
                    .resource::<MaterialLibrary>()
                    .materials
                    .iter()
                    .map(|def| def.id)
                    .find(|id| !used.contains(id))
            };
            match unused {
                Some(id) => {
                    world.resource_mut::<MaterialEditorState>().target = Some(id);
                    let before = world.resource::<MaterialLibrary>().materials.len();
                    world.resource_mut::<MaterialProbe>().library_before = before;
                    invoke(world, "material.delete");
                }
                None => check(world, false, "an unused material exists to delete"),
            }
        }
        1220 => {
            let before = world.resource::<MaterialProbe>().library_before;
            let after = world.resource::<MaterialLibrary>().materials.len();
            check(
                world,
                after == before - 1,
                &format!("delete removed the unused material ({before} then {after})"),
            );
            shot(world, "61-material-verbs");
        }
        // ── Inheritance: one edit instead of N ────────────────────────────
        1240 => {
            let first = world
                .resource::<MaterialLibrary>()
                .materials
                .first()
                .map(|def| def.id);
            world.resource_mut::<MaterialEditorState>().target = first;
            world.resource_mut::<MaterialProbe>().target_before = first;
            invoke(world, "material.new-instance");
        }
        1280 => {
            let base = world.resource::<MaterialProbe>().target_before;
            let instance = world.resource::<MaterialEditorState>().target;
            let follows = instance
                .and_then(|id| world.resource::<MaterialLibrary>().get(&id).cloned())
                .map(|def| def.base);
            check(
                world,
                follows == Some(base) && instance != base,
                &format!("the instance follows its base ({follows:?})"),
            );
            // It owns nothing yet, so it IS its base.
            let (resolved, base_def) = {
                let library = world.resource::<MaterialLibrary>();
                (
                    instance.and_then(|id| library.resolved(&id)),
                    base.and_then(|id| library.resolved(&id)),
                )
            };
            check(
                world,
                resolved.as_ref().map(|def| def.roughness)
                    == base_def.as_ref().map(|d| d.roughness)
                    && resolved.as_ref().map(|def| def.base_color)
                        == base_def.as_ref().map(|d| d.base_color),
                "a fresh instance looks exactly like its base",
            );
        }
        // Edit the BASE and the instance follows, in the same frame.
        1320 => {
            let base = world.resource::<MaterialProbe>().target_before;
            if let Some(base) = base
                && let Some(def) = world.resource_mut::<MaterialLibrary>().get_mut(&base)
            {
                def.roughness = 0.123;
            }
        }
        1340 => {
            let instance = world.resource::<MaterialEditorState>().target;
            let followed =
                instance.and_then(|id| world.resource::<MaterialLibrary>().resolved(&id));
            check(
                world,
                followed.is_some_and(|def| (def.roughness - 0.123).abs() < 1e-4),
                "a base edit reached the instance without touching it",
            );
        }
        // Claim ONE field on the instance; the rest still follows.
        1380 => slider_change(world, Field::Metallic, 0.75, true),
        1420 => {
            let instance = world.resource::<MaterialEditorState>().target;
            let (stored, resolved) = {
                let library = world.resource::<MaterialLibrary>();
                (
                    instance.and_then(|id| library.get(&id).cloned()),
                    instance.and_then(|id| library.resolved(&id)),
                )
            };
            check(
                world,
                stored.as_ref().is_some_and(|def| {
                    def.overridden
                        .contains(&editor_scene::materials::MaterialField::Metallic)
                }),
                "editing a field CLAIMED it for the instance",
            );
            check(
                world,
                resolved.as_ref().is_some_and(|def| {
                    (def.metallic - 0.75).abs() < 1e-4 && (def.roughness - 0.123).abs() < 1e-4
                }),
                "the claimed field is its own and the rest still follows",
            );
            shot(world, "62-material-inheritance");
        }
        // The panel has to SAY which fields are claimed, or the inheritance is
        // real and invisible — readable only in the file.
        1430 => {
            world.resource_mut::<MaterialEditorState>().open = true;
            world.resource_mut::<MaterialEditorState>().refresh = true;
        }
        1450 => {
            let reverts: Vec<editor_scene::materials::MaterialField> = world
                .query::<&crate::material_editor::RevertField>()
                .iter(world)
                .map(|revert| revert.0)
                .collect();
            check(
                world,
                reverts == vec![editor_scene::materials::MaterialField::Metallic],
                &format!("exactly the claimed field offers a revert ({reverts:?})"),
            );
            // Give it back: the value becomes whatever the base says now.
            let glyph = world
                .query_filtered::<Entity, With<crate::material_editor::RevertField>>()
                .iter(world)
                .next();
            match glyph {
                Some(entity) => {
                    let center = crate::probe_handson::ui_center(world, entity)
                        .unwrap_or(Vec2::new(10.0, 10.0));
                    move_cursor(world, center);
                    click(world, true);
                    click(world, false);
                }
                None => check(world, false, "a revert affordance exists to press"),
            }
        }
        1470 => {
            let instance = world.resource::<MaterialEditorState>().target;
            let (stored, resolved) = {
                let library = world.resource::<MaterialLibrary>();
                (
                    instance.and_then(|id| library.get(&id).cloned()),
                    instance.and_then(|id| library.resolved(&id)),
                )
            };
            check(
                world,
                stored.as_ref().is_some_and(|def| def.overridden.is_empty()),
                "reverting gave the field back to the base",
            );
            let base_metallic = stored
                .as_ref()
                .and_then(|def| def.base)
                .and_then(|base| world.resource::<MaterialLibrary>().resolved(&base))
                .map(|def| def.metallic);
            check(
                world,
                resolved.map(|def| def.metallic) == base_metallic,
                "and the value is the base's again",
            );
            shot(world, "63-material-inherited-rows");
        }
        // Detaching keeps the look and stops the following. Capture what it
        // looks like FIRST: the assertion is "unchanged", not a fixed number.
        1490 => {
            let looked_like = world
                .resource::<MaterialEditorState>()
                .target
                .and_then(|id| world.resource::<MaterialLibrary>().resolved(&id));
            world.resource_mut::<MaterialProbe>().before_detach = looked_like;
            invoke(world, "material.detach");
        }
        1500 => {
            let instance = world.resource::<MaterialEditorState>().target;
            let stored =
                instance.and_then(|id| world.resource::<MaterialLibrary>().get(&id).cloned());
            check(
                world,
                stored.as_ref().is_some_and(|def| def.base.is_none()),
                "detach stopped the following",
            );
            let looked_like = world.resource::<MaterialProbe>().before_detach.clone();
            let unchanged = match (&stored, &looked_like) {
                (Some(now), Some(before)) => {
                    (now.roughness - before.roughness).abs() < 1e-5
                        && (now.metallic - before.metallic).abs() < 1e-5
                        && now.base_color == before.base_color
                        && now.unlit == before.unlit
                        && now.textures == before.textures
                }
                _ => false,
            };
            check(
                world,
                unchanged,
                "and baked in exactly what it looked like, field for field",
            );
        }
        // ── Picking a texture by NAME, not by cycling ─────────────────────
        1540 => {
            // Press the normal-map chip: five slots each cycling blindly
            // through every imported texture is guessing, not picking.
            world.resource_mut::<MaterialEditorState>().open = true;
            world.resource_mut::<MaterialEditorState>().refresh = true;
        }
        1550 => {
            // The texture rows live below the fold of a sixteen-row panel, so
            // scroll to them first — the press has to be a real press on a
            // real, visible chip for this to be worth anything.
            let bodies: Vec<Entity> = world
                .query_filtered::<Entity, With<crate::material_editor::MaterialEditorBody>>()
                .iter(world)
                .collect();
            for body in bodies {
                if let Some(mut scroll) = world.get_mut::<ScrollPosition>(body) {
                    scroll.y = 4000.0;
                }
            }
        }
        1560 => {
            let chip = world
                .query::<(Entity, &Field)>()
                .iter(world)
                .find(|(_, field)| **field == Field::Texture(TextureSlot::Normal))
                .map(|(entity, _)| entity);
            match chip {
                Some(entity) => {
                    let center = crate::probe_handson::ui_center(world, entity)
                        .unwrap_or(Vec2::new(10.0, 10.0));
                    move_cursor(world, center);
                    click(world, true);
                    click(world, false);
                }
                None => check(world, false, "the normal slot has a chip to press"),
            }
        }
        1600 => {
            let open = world.resource::<crate::palette::PaletteState>().open;
            check(world, open, "a texture chip opens the picker");
            // Type part of the name: the point is finding one, not walking past
            // the others until it comes round.
            for (code, ch) in [
                (KeyCode::KeyS, "s"),
                (KeyCode::KeyW, "w"),
                (KeyCode::KeyA, "a"),
            ] {
                tap(world, code, ch);
            }
        }
        1640 => tap_named(world, KeyCode::Enter, Key::Enter),
        1700 => {
            let bound = world
                .resource::<MaterialEditorState>()
                .target
                .and_then(|id| world.resource::<MaterialLibrary>().get(&id).cloned())
                .and_then(|def| def.texture(TextureSlot::Normal));
            check(
                world,
                bound.is_some(),
                &format!("the searched texture landed in the NORMAL slot ({bound:?})"),
            );
            shot(world, "64-material-texture-picker");
        }
        // ── The room the surface is judged in ─────────────────────────────
        // Bevy PANICS if the source cubemap is not square power-of-two, and
        // only replaces the generated light with a real EnvironmentMapLight
        // once it has filtered it — so this check proves the cubemap was
        // accepted and prefiltered, not merely that a component was inserted.
        1720 => {
            let rig = world.resource::<MaterialPreviewRig>().camera;
            let generated = world
                .get::<bevy::light::GeneratedEnvironmentMapLight>(rig)
                .is_some();
            let filtered = world.get::<bevy::light::EnvironmentMapLight>(rig).is_some();
            check(
                world,
                generated || filtered,
                "the material preview stands in an environment",
            );
            check(
                world,
                filtered,
                "and Bevy filtered it, so roughness has mips to blur through",
            );
        }
        1760 => {
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
