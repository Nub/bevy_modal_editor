//! The OWNER HANDS-ON checklist, automated (HANDSON_PROBE=1, M4-ACCEPTANCE):
//! the parts no other probe covers — a TEXTURED material end-to-end with undo
//! (real chip click through UI picking), per-instance override surviving a
//! template edit that propagates to non-overriding instances, socket
//! AUTHORING via the add-component flow, and play/reset with authored content
//! intact. Every interaction goes through real input or real widget events.

use bevy::input::keyboard::Key;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use editor_core::prelude::*;
use editor_prefabs::{PrefabInstance, PrefabLibrary};
use editor_scene::PrefabStamped;
use editor_scene::materials::MaterialLibrary;
use editor_scene::models::{EntryKind, MeshRef, ModelLibrary};
use uuid::Uuid;

use crate::material_editor::{Field, MaterialPreviewRig};
use crate::probe_user::{click, key, move_cursor, shot, tap, tap_named};

#[derive(Resource, Default)]
pub(crate) struct HandsonProbe {
    frame: u32,
    failures: Vec<String>,
    material: Option<Uuid>,
    texture: Option<Uuid>,
    /// (instance root, member world position) captured before edits.
    members_before: Vec<(Entity, Vec3)>,
    overridden_root: Option<Entity>,
    edited_root: Option<Entity>,
    /// The entity an inspector field addresses, and its transform before the
    /// edit — a patched leaf must leave every sibling field untouched.
    patched: Option<(Entity, Transform)>,
    /// Undo depth and dirty flag before a hide, so the probe can prove hide is
    /// not an edit.
    before_hide: Option<(usize, bool)>,
    /// Undo depth, drum count and the source pose before an array.
    array_before: Option<(usize, usize, Transform)>,
    /// Poses before a move, to prove a carried child did not move itself.
    move_before: Vec<(Entity, Transform)>,
    /// The parent and children a delete is about to take, plus their scene ids
    /// (undo hands back NEW entities for the same ids).
    delete_subject: Option<(Entity, Vec<Entity>)>,
    delete_ids: Vec<SceneId>,
    /// Which drums existed before an array, so the copies can be identified by
    /// difference rather than by guessing at the level's own spacing.
    array_roots_before: Vec<Entity>,
    /// World poses before a mirror, to check the reflection exactly.
    mirror_before: Vec<(Entity, Transform)>,
    /// Where the locked objects were before an edit was aimed at them.
    locked_before: Vec<Vec3>,
}

fn check(world: &mut World, ok: bool, what: &str) {
    if ok {
        info!("HANDSON-PROBE PASS: {what}");
    } else {
        error!("HANDSON-PROBE FAIL: {what}");
        world
            .resource_mut::<HandsonProbe>()
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

pub(crate) fn type_word(world: &mut World, word: &str) {
    for ch in word.chars() {
        let code = match ch {
            'a' => KeyCode::KeyA,
            'b' => KeyCode::KeyB,
            'c' => KeyCode::KeyC,
            'd' => KeyCode::KeyD,
            'e' => KeyCode::KeyE,
            'f' => KeyCode::KeyF,
            'g' => KeyCode::KeyG,
            'h' => KeyCode::KeyH,
            'i' => KeyCode::KeyI,
            'j' => KeyCode::KeyJ,
            'k' => KeyCode::KeyK,
            'l' => KeyCode::KeyL,
            'm' => KeyCode::KeyM,
            'n' => KeyCode::KeyN,
            'o' => KeyCode::KeyO,
            'p' => KeyCode::KeyP,
            'q' => KeyCode::KeyQ,
            'r' => KeyCode::KeyR,
            's' => KeyCode::KeyS,
            't' => KeyCode::KeyT,
            'u' => KeyCode::KeyU,
            'v' => KeyCode::KeyV,
            'w' => KeyCode::KeyW,
            'x' => KeyCode::KeyX,
            'y' => KeyCode::KeyY,
            'z' => KeyCode::KeyZ,
            _ => continue,
        };
        tap(world, code, &ch.to_string());
    }
}

/// Logical-pixel center of a UI node — drives REAL cursor clicks on chrome.
pub(crate) fn ui_center(world: &mut World, entity: Entity) -> Option<Vec2> {
    let physical = world
        .get::<bevy::ui::UiGlobalTransform>(entity)
        .map(|t| t.translation)?;
    let scale = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .iter(world)
        .next()
        .map(|w| w.scale_factor())?;
    Some(Vec2::new(physical.x, physical.y) / scale)
}

/// Center of the first hierarchy row whose label matches (and whose ID column
/// avoids `exclude_short`) — rows are clicked like a user would (also the
/// first real coverage of row clicking).
fn hierarchy_row_center(
    world: &mut World,
    label: &str,
    exclude_short: Option<&str>,
    require_short: Option<&str>,
) -> Option<Vec2> {
    let rows: Vec<Entity> = {
        let mut query = world.query::<(Entity, &crate::hierarchy::HierarchyRow)>();
        let mut rows: Vec<(usize, Entity)> = query.iter(world).map(|(e, row)| (row.0, e)).collect();
        rows.sort_by_key(|(index, _)| *index);
        rows.into_iter().map(|(_, e)| e).collect()
    };
    for row in rows {
        let mut stack = vec![row];
        let mut matched = false;
        let mut excluded = false;
        let mut required = require_short.is_none();
        while let Some(entity) = stack.pop() {
            if let Some(text) = world.get::<Text>(entity) {
                if text.0 == label {
                    matched = true;
                }
                if exclude_short.is_some_and(|short| text.0 == short) {
                    excluded = true;
                }
                if require_short.is_some_and(|short| text.0 == short) {
                    required = true;
                }
            }
            if let Some(children) = world.get::<Children>(entity) {
                stack.extend(children.iter());
            }
        }
        if matched && !excluded && required {
            return ui_center(world, row);
        }
    }
    None
}

/// Where an entity actually IS on screen. Clicking a fixed fraction of the
/// window only worked while the drums happened to project over that spot: it
/// missed by a few pixels the moment frame timing shifted the placement, which
/// made an unrelated change look like a selection bug.
pub(crate) fn screen_position_of(world: &mut World, target: Vec3) -> Option<Vec2> {
    let (camera, camera_transform) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<Camera3d>>()
        .iter(world)
        // The material preview rig has its own active camera rendering to a
        // texture; only the one drawing the VIEWPORT can be projected through.
        .max_by_key(|(camera, _)| camera.order)
        .map(|(camera, transform)| (camera.clone(), *transform))?;
    camera.world_to_viewport(&camera_transform, target).ok()
}

/// A scene root that really has scene children — the flattened model. Prefab
/// instances do not count: their members are DERIVED and deliberately outside
/// the capture, so deleting one would prove nothing about subtree restore.
fn scene_root_with_scene_children(world: &mut World) -> Option<(Entity, Vec<Entity>)> {
    let roots: Vec<Entity> = world
        .query_filtered::<Entity, (With<SceneId>, Without<ChildOf>)>()
        .iter(world)
        .collect();
    for root in roots {
        let children: Vec<Entity> = world
            .get::<Children>(root)
            .map(|kids| {
                kids.iter()
                    .filter(|child| {
                        world.get::<SceneId>(*child).is_some()
                            && world.get::<PrefabStamped>(*child).is_none()
                    })
                    .collect()
            })
            .unwrap_or_default();
        if children.len() >= 2 {
            return Some((root, children));
        }
    }
    None
}
/// The current selection, as a set to test membership against.
fn selected_entities(world: &mut World) -> Vec<Entity> {
    world
        .query_filtered::<Entity, With<Selected>>()
        .iter(world)
        .collect()
}

fn flash_text(world: &mut World) -> String {
    world
        .resource::<crate::statusbar::StatusFlash>()
        .text
        .clone()
}

/// Scene roots with this `Name` — the fixture primitives.
fn named_roots(world: &mut World, name: &str) -> Vec<Entity> {
    let wanted = name.to_string();
    let mut rows: Vec<(Entity, Vec3)> = world
        .query_filtered::<(Entity, &Name, &Transform), (With<SceneId>, Without<ChildOf>)>()
        .iter(world)
        .filter(|(_, n, _)| n.as_str() == wanted)
        .map(|(e, _, t)| (e, t.translation))
        .collect();
    rows.sort_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.1.z.total_cmp(&b.1.z)));
    rows.into_iter().map(|(e, _)| e).collect()
}

/// Is a mesh-bearing descendant of `root` actually being drawn?
///
/// Asked of the SUBTREE, never the root: a prefab instance root carries no
/// `Mesh3d` at all, so "is the root hidden" would pass with nothing hidden.
/// `None` = no mesh under there to judge.
fn mesh_descendant_visible(world: &mut World, root: Entity) -> Option<bool> {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if world.get::<Mesh3d>(entity).is_some()
            && let Some(inherited) = world.get::<InheritedVisibility>(entity)
        {
            return Some(inherited.get());
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    None
}

fn subtree_is_outlined(world: &mut World, root: Entity) -> bool {
    let mut stack = vec![root];
    while let Some(entity) = stack.pop() {
        if world
            .get::<bevy_outliner::prelude::MeshOutline>(entity)
            .is_some()
        {
            return true;
        }
        if let Some(children) = world.get::<Children>(entity) {
            stack.extend(children.iter());
        }
    }
    false
}

/// Where an object sits on screen, for a real click.
fn screen_of(world: &mut World, entity: Entity) -> Option<Vec2> {
    let at = world.get::<GlobalTransform>(entity)?.translation();
    let (camera, transform) = world
        .query::<(
            &Camera,
            &GlobalTransform,
            Option<&bevy::camera::RenderTarget>,
        )>()
        .iter(world)
        .find(|(camera, _, target)| editor_core::camera::is_viewport_camera(camera, *target))
        .map(|(c, t, _)| (c.clone(), *t))?;
    camera.world_to_viewport(&transform, at).ok()
}

/// The drum prefab instances, left to right.
fn drum_roots(world: &mut World) -> Vec<Entity> {
    let mut roots: Vec<(Entity, Vec3)> = world
        .query_filtered::<(Entity, &Transform, &Name), (With<PrefabInstance>, Without<PrefabStamped>)>()
        .iter(world)
        .filter(|(.., name)| name.as_str() == "drum")
        .map(|(e, t, _)| (e, t.translation))
        .collect();
    roots.sort_by(|a, b| a.1.x.total_cmp(&b.1.x).then(a.1.z.total_cmp(&b.1.z)));
    roots.into_iter().map(|(e, _)| e).collect()
}

/// The stamped MEMBER (SceneId child) under a drum instance root.
fn member_of(world: &mut World, root: Entity) -> Option<Entity> {
    let children = world.get::<Children>(root)?;
    children
        .iter()
        .find(|child| world.get::<PrefabStamped>(*child).is_some())
}

pub(crate) fn probe_handson(world: &mut World) {
    world.resource_mut::<HandsonProbe>().frame += 1;
    let frame = world.resource::<HandsonProbe>().frame;
    if frame == 1 {
        let _ = std::fs::create_dir_all(crate::probe_user::SHOT_DIR);
        // Clean slate: probe-owned prefabs and sources never leak between runs.
        for stale in ["drum"] {
            let dir = editor_prefabs::authoring::prefabs_dir();
            let _ = std::fs::remove_file(dir.join(format!("{stale}.prefab.ron")));
            let _ = std::fs::remove_file(dir.join(format!("{stale}.prefab.ron.bak")));
            let mut library = world.resource_mut::<PrefabLibrary>();
            let ids: Vec<_> = library
                .prefabs
                .iter()
                .filter(|(_, def)| def.name.eq_ignore_ascii_case(stale))
                .map(|(id, _)| *id)
                .collect();
            for id in ids {
                library.prefabs.remove(&id);
            }
        }
        let root = world.resource::<ModelLibrary>().fs_root.clone();
        let models = root.join(editor_scene::models::MODELS_DIR);
        let textures = root.join(editor_scene::models::TEXTURES_DIR);
        let _ = std::fs::create_dir_all(&models);
        let _ = std::fs::create_dir_all(&textures);
        std::fs::write(
            models.join("barrel.glb"),
            editor_assets::fixture::barrel_glb(1.0),
        )
        .unwrap();
        std::fs::write(
            textures.join("swatch.png"),
            editor_assets::fixture::swatch_png(),
        )
        .unwrap();
        info!("HANDSON-PROBE armed");
    }
    match frame {
        60 => tap_named(world, KeyCode::Enter, Key::Enter),
        120 => invoke(world, "core.toggle-editor"),
        160 => invoke(world, "asset.import"),
        200 => {
            let texture = world
                .resource::<ModelLibrary>()
                .entries
                .iter()
                .find(|e| e.kind == EntryKind::Texture && e.name == "swatch")
                .map(|e| e.uuid);
            check(world, texture.is_some(), "swatch.png imported as a texture");
            world.resource_mut::<HandsonProbe>().texture = texture;
        }
        // ── Checklist: build a TEXTURED material end-to-end with undo ──────
        220 => invoke(world, "material.new"),
        240 => {
            world.resource_mut::<HandsonProbe>().material = world
                .resource::<MaterialLibrary>()
                .materials
                .last()
                .map(|def| def.id);
            invoke(world, "material.edit");
        }
        // The editor scrolls: the texture slot sits below the fold on a short
        // window, so reach it the way a user does — scroll, then aim.
        290 => {
            let body = world
                .query_filtered::<Entity, With<crate::material_editor::MaterialEditorBody>>()
                .iter(world)
                .next();
            if let Some(body) = body
                && let Some(mut scroll) = world.get_mut::<bevy::ui::ScrollPosition>(body)
            {
                scroll.y = f32::MAX; // layout clamps to the real extent
            }
        }
        300 => {
            let chip = world
                .query::<(Entity, &Field)>()
                .iter(world)
                .find(|(_, f)| matches!(f, Field::Texture(_)))
                .map(|(e, _)| e);
            match chip.and_then(|c| ui_center(world, c)) {
                Some(center) => move_cursor(world, center),
                None => check(world, false, "texture chip locatable on screen"),
            }
        }
        310 => click(world, true),
        312 => click(world, false),
        // The chip opens the PICKER now rather than cycling blindly through
        // every imported texture — so choose the one wanted, by name.
        330 => {
            let open = world.resource::<crate::palette::PaletteState>().open;
            check(world, open, "the texture chip opens the picker");
            type_word(world, "swa");
        }
        // The texture chip is LIVE right now (the picker is filtered to
        // "swa"). Both sibling preview arms were mis-parented — the texture
        // sphere sat at the world origin 900 units from the preview camera,
        // and the material sphere was translated to preview home a second time
        // under a root already there — so both panes rendered nothing at all.
        // Nobody noticed because the only preview assertion in this probe reads
        // the MATERIAL EDITOR's rig, which is a different camera entirely.
        350 => {
            let meshes = crate::palette_preview::preview_mesh_count(world);
            check(
                world,
                meshes >= 1,
                &format!("the texture picker actually shows the texture ({meshes} meshes)"),
            );
        }
        360 => tap_named(world, KeyCode::Enter, Key::Enter),
        380 => {
            let (probe_material, probe_texture) = {
                let probe = world.resource::<HandsonProbe>();
                (probe.material, probe.texture)
            };
            let textured = probe_material
                .and_then(|id| world.resource::<MaterialLibrary>().get(&id).cloned())
                .and_then(|def| def.texture(editor_scene::materials::TextureSlot::BaseColor));
            // ANY imported texture counts: real projects have their own
            // textures in the tree and the cycle starts from the first —
            // probes must survive live project content.
            let _ = probe_texture;
            let is_imported = textured.is_some_and(|uuid| {
                world
                    .resource::<ModelLibrary>()
                    .entries
                    .iter()
                    .any(|e| e.kind == EntryKind::Texture && e.uuid == uuid)
            });
            check(
                world,
                is_imported,
                "clicking the texture chip bound an imported texture",
            );
            let preview_textured = {
                let handle = world.resource::<MaterialPreviewRig>().material.clone();
                world
                    .resource::<Assets<StandardMaterial>>()
                    .get(&handle)
                    .is_some_and(|m| m.base_color_texture.is_some())
            };
            check(world, preview_textured, "the preview renders the texture");
            shot(world, "22-textured-material");
        }
        400 => invoke(world, "core.undo"),
        // And the material chip, the other mis-parented arm. AFTER the undo
        // settles: opening the palette across an undo step disturbs it.
        452 => invoke(world, "material.assign"),
        462 => {
            let meshes = crate::palette_preview::preview_mesh_count(world);
            check(
                world,
                meshes >= 1,
                &format!("the material palette actually shows the material ({meshes} meshes)"),
            );
        }
        466 => tap_named(world, KeyCode::Escape, Key::Escape),
        440 => {
            let cleared = world
                .resource::<HandsonProbe>()
                .material
                .and_then(|id| world.resource::<MaterialLibrary>().get(&id).cloned())
                .is_some_and(|def| {
                    def.texture(editor_scene::materials::TextureSlot::BaseColor)
                        .is_none()
                });
            check(world, cleared, "asset undo removed the texture binding");
            invoke(world, "core.redo");
        }
        480 => {
            let restored = world
                .resource::<HandsonProbe>()
                .material
                .and_then(|id| world.resource::<MaterialLibrary>().get(&id).cloned())
                .is_some_and(|def| {
                    def.texture(editor_scene::materials::TextureSlot::BaseColor)
                        .is_some()
                });
            check(world, restored, "redo restored the texture binding");
        }
        500 => tap_named(world, KeyCode::Escape, Key::Escape),
        // Stale probe artifacts: play-mode SAVES the scene, so a previous
        // run's drums and socket-barrel live in level.ron — despawn them
        // through the kernel before this run's placements.
        505 => {
            let stale: Vec<SceneId> = {
                let mut found = Vec::new();
                let mut roots = world.query_filtered::<(
                    &SceneId,
                    &Name,
                    Option<&MeshRef>,
                    Option<&PrefabInstance>,
                ), Without<PrefabStamped>>();
                for (id, name, mesh_ref, instance) in roots.iter(world) {
                    let probe_owned = (mesh_ref.is_some() && name.as_str() == "barrel")
                        || (instance.is_some() && name.as_str() == "drum");
                    if probe_owned {
                        found.push(*id);
                    }
                }
                found
            };
            if !stale.is_empty() {
                info!(
                    "HANDSON-PROBE cleanup: despawning {} stale artifacts",
                    stale.len()
                );
                world.resource_mut::<EditQueue>().0.push(Transaction {
                    label: "probe cleanup".into(),
                    gesture: None,
                    ops: stale.into_iter().map(|id| Op::Despawn { id }).collect(),
                });
            }
        }
        // ── Checklist: prefab from the import; overrides vs template edits ─
        520 => tap(world, KeyCode::KeyI, "i"),
        550 => type_word(world, "barr"),
        580 => tap_named(world, KeyCode::Enter, Key::Enter),
        760 => {
            let placed = world.query::<&MeshRef>().iter(world).count() == 1;
            check(world, placed, "barrel placed from the palette");
        }
        780 => tap(world, KeyCode::KeyG, "g"),
        810 => type_word(world, "drum"),
        840 => tap_named(world, KeyCode::Enter, Key::Enter),
        // Park the cursor on CLEAR ground (the same strip probe_user trusts):
        // every palette placement lands exactly there, deterministically.
        870 => {
            let center = crate::probe_user::viewport_center(world);
            move_cursor(world, center);
        }
        880 => tap(world, KeyCode::KeyW, "w"),
        885 => tap(world, KeyCode::KeyX, "x"),
        890 => tap(world, KeyCode::Minus, "-"),
        892 => tap(world, KeyCode::Digit1, "1"),
        894 => tap(world, KeyCode::Period, "."),
        896 => tap(world, KeyCode::Digit5, "5"),
        900 => tap_named(world, KeyCode::Enter, Key::Enter),
        920 => tap_named(world, KeyCode::Escape, Key::Escape),
        940 => tap(world, KeyCode::KeyI, "i"),
        970 => type_word(world, "drum"),
        1000 => tap_named(world, KeyCode::Enter, Key::Enter),
        1020 => tap(world, KeyCode::KeyW, "w"),
        1025 => tap(world, KeyCode::KeyX, "x"),
        1030 => tap(world, KeyCode::Digit1, "1"),
        1035 => tap(world, KeyCode::Period, "."),
        1040 => tap(world, KeyCode::Digit5, "5"),
        1050 => tap_named(world, KeyCode::Enter, Key::Enter),
        1100 => tap_named(world, KeyCode::Escape, Key::Escape),
        1120 => tap(world, KeyCode::KeyI, "i"),
        1150 => type_word(world, "drum"),
        1180 => tap_named(world, KeyCode::Enter, Key::Enter),
        // Drum 3 STAYS at the placement point — the cursor is already on it.
        1270 => tap_named(world, KeyCode::Escape, Key::Escape),
        1300 => {
            let roots = drum_roots(world);
            check(world, roots.len() == 3, "three drum instances placed");
            let members: Vec<(Entity, Vec3)> = roots
                .iter()
                .filter_map(|root| {
                    let member = member_of(world, *root)?;
                    let position = world.get::<GlobalTransform>(member)?.translation();
                    Some((*root, position))
                })
                .collect();
            check(world, members.len() == 3, "every drum stamped its member");
            world.resource_mut::<HandsonProbe>().members_before = members;
        }
        // Override: click the drum the cursor was parked on — by where it
        // actually projects, so the test survives a shift in frame timing.
        1320 => {
            let target = world
                .resource::<HandsonProbe>()
                .members_before
                .last()
                .map(|(_, at)| *at + Vec3::Y * 0.3)
                .and_then(|at| screen_position_of(world, at))
                .unwrap_or_else(|| crate::probe_user::viewport_center(world));
            move_cursor(world, target);
        }

        1330 => click(world, true),
        1332 => click(world, false),
        // Owner rule: a prefab selects as a UNIT until opened. The click lands
        // on the INSTANCE, and authoring on a member is a deliberate step in.
        1336 => {
            let picked_root = world
                .query_filtered::<(), (With<Selected>, With<PrefabInstance>)>()
                .iter(world)
                .count();
            let picked_member = world
                .query_filtered::<(), (With<Selected>, With<PrefabStamped>)>()
                .iter(world)
                .count();
            check(
                world,
                picked_root == 1 && picked_member == 0,
                "clicking a sealed prefab selects the INSTANCE, not the member",
            );
            invoke(world, "prefab.open");
        }
        1342 => {
            let target = world
                .resource::<HandsonProbe>()
                .members_before
                .last()
                .map(|(_, at)| *at + Vec3::Y * 0.3)
                .and_then(|at| screen_position_of(world, at))
                .unwrap_or_else(|| crate::probe_user::viewport_center(world));
            move_cursor(world, target);
        }
        1345 => click(world, true),
        1347 => click(world, false),
        1350 => {
            let member = world
                .query_filtered::<Entity, (With<Selected>, With<PrefabStamped>)>()
                .iter(world)
                .next();
            check(
                world,
                member.is_some(),
                "once OPEN, clicking the mesh selects the member",
            );
            // The selection outline must reach DERIVED meshes (imported models
            // carry no Mesh3d themselves — the gltf children silhouette).
            let outlined_derived = {
                let mut found = false;
                if let Some(member) = member {
                    let mut stack = vec![member];
                    while let Some(entity) = stack.pop() {
                        if world
                            .get::<bevy_outliner::prelude::MeshOutline>(entity)
                            .is_some()
                        {
                            found = true;
                            break;
                        }
                        if let Some(children) = world.get::<Children>(entity) {
                            stack.extend(children.iter());
                        }
                    }
                }
                found
            };
            check(
                world,
                outlined_derived,
                "the selection outline reaches the model's derived meshes",
            );
            let root = member.and_then(|m| world.get::<ChildOf>(m).map(|c| c.parent()));
            world.resource_mut::<HandsonProbe>().overridden_root = root;
        }
        // Member OVERRIDES author through the INSPECTOR (gesture moves on a
        // stamped selection deliberately move the whole instance): commit
        // translation.y = 1 through the real field event.
        1360 => {
            // An inspector edit is a PATCH on one leaf, so record the WHOLE
            // transform of the entity the field addresses: the siblings have to
            // come through untouched.
            let field = world
                .query::<(Entity, &crate::inspector::InspectorField)>()
                .iter(world)
                .find(|(_, f)| f.path == "translation.y")
                .map(|(entity, f)| (entity, f.target));
            match field {
                Some((source, target)) => {
                    let subject = world
                        .resource::<editor_api::edits::SceneIndex>()
                        .get(&target);
                    world.resource_mut::<HandsonProbe>().patched = subject
                        .and_then(|entity| world.get::<Transform>(entity).copied())
                        .map(|transform| (subject.unwrap(), transform));
                    world.trigger(bevy::ui_widgets::ValueChange {
                        source,
                        value: 1.0f32,
                        is_final: true,
                    });
                }
                None => check(world, false, "inspector Y field present for the member"),
            }
        }
        1370 => {
            let recorded = world.resource::<HandsonProbe>().patched;
            let before = recorded.map(|(_, transform)| transform);
            let after = recorded.and_then(|(entity, _)| world.get::<Transform>(entity).copied());
            match (before, after) {
                (Some(before), Some(after)) => {
                    check(
                        world,
                        (after.translation.y - 1.0).abs() < 1e-4,
                        &format!("the edited leaf took the value ({})", after.translation.y),
                    );
                    // Field granularity, end to end: editing y through the
                    // inspector must not carry x, z, the rotation or the scale
                    // along with it.
                    let siblings_held = (after.translation.x - before.translation.x).abs() < 1e-6
                        && (after.translation.z - before.translation.z).abs() < 1e-6
                        && after.scale.abs_diff_eq(before.scale, 1e-6)
                        && after.rotation.abs_diff_eq(before.rotation, 1e-6);
                    check(
                        world,
                        siblings_held,
                        "editing one field left every other field of the component alone",
                    );
                }
                _ => check(world, false, "the member's transform is readable"),
            }
        }
        1400 => tap_named(world, KeyCode::Escape, Key::Escape),
        // Template edit through the HIERARCHY: click the drum row (real row
        // click), Enter opens the instance, click its member row, typed move.
        1420 => {
            // NOT the overridden drum: opening it would bake the override
            // into the template and void the survival check.
            let exclude = world
                .resource::<HandsonProbe>()
                .overridden_root
                .and_then(|root| world.get::<SceneId>(root))
                .map(|id| id.0.to_string()[..8].to_string());
            match hierarchy_row_center(world, "drum", exclude.as_deref(), None) {
                Some(center) => move_cursor(world, center),
                None => check(world, false, "a drum row exists in the hierarchy"),
            }
        }
        1430 => click(world, true),
        1432 => click(world, false),
        1460 => tap_named(world, KeyCode::Enter, Key::Enter),
        1490 => {
            let opened_root = world
                .resource::<editor_prefabs::open_mode::OpenInstance>()
                .0
                .as_ref()
                .map(|open| open.root)
                .and_then(|id| world.resource::<SceneIndex>().get(&id));
            check(
                world,
                opened_root.is_some(),
                "row click + Enter opened the instance",
            );
            world.resource_mut::<HandsonProbe>().edited_root = opened_root;
        }
        1510 => {
            // THE opened drum's member, matched by its ID column — the first
            // 'barrel' row could belong to another (out-of-scope) instance.
            let member_short = world
                .resource::<HandsonProbe>()
                .edited_root
                .and_then(|root| member_of(world, root))
                .and_then(|member| world.get::<SceneId>(member))
                .map(|id| id.0.to_string()[..8].to_string());
            match hierarchy_row_center(world, "barrel", None, member_short.as_deref()) {
                Some(center) => move_cursor(world, center),
                None => check(world, false, "the member row exists in the open scope"),
            }
        }
        1520 => click(world, true),
        1522 => click(world, false),
        1560 => tap(world, KeyCode::KeyW, "w"),
        1565 => tap(world, KeyCode::KeyX, "x"),
        1570 => tap(world, KeyCode::Digit1, "1"),
        1580 => tap_named(world, KeyCode::Enter, Key::Enter),
        1600 => tap_named(world, KeyCode::Escape, Key::Escape),
        1620 => tap_named(world, KeyCode::Escape, Key::Escape),
        // Verdict: non-overriding instances follow; the override survives.
        1680 => {
            let before = world.resource::<HandsonProbe>().members_before.clone();
            let overridden = world.resource::<HandsonProbe>().overridden_root;
            let edited = world.resource::<HandsonProbe>().edited_root;
            let mut followed = 0usize;
            let mut override_intact = false;
            for (root, before_pos) in &before {
                let Some(member) = member_of(world, *root) else {
                    continue;
                };
                let Some(now) = world
                    .get::<GlobalTransform>(member)
                    .map(|t| t.translation())
                else {
                    continue;
                };
                if Some(*root) == overridden {
                    // Per-field override semantics (D5): the overridden Y pins
                    // to the user's value while the un-overridden X still
                    // follows the template edit.
                    override_intact =
                        (now.y - 1.0).abs() < 0.01 && (now.x - (before_pos.x + 1.0)).abs() < 0.01;
                } else if (now.x - (before_pos.x + 1.0)).abs() < 0.01 {
                    followed += 1;
                }
                let _ = edited;
            }
            check(
                world,
                followed == 2,
                &format!("template edit propagated to non-overriding instances ({followed}/2)"),
            );
            check(
                world,
                override_intact,
                "the member override survived the template edit",
            );
            shot(world, "23-override-vs-template");
        }
        // ── Checklist: author a socket via the add-component flow ──────────
        1700 => tap_named(world, KeyCode::Escape, Key::Escape),
        1720 => tap(world, KeyCode::KeyI, "i"),
        1750 => type_word(world, "barr"),
        1780 => tap_named(world, KeyCode::Enter, Key::Enter),
        // A freshly placed model is a LINKED reference, and components may not
        // be added to one (owner rule) — its geometry lives in derived gltf
        // children, so the component would land on an entity with no mesh.
        // Flattening is the documented way to make the import authorable, and
        // it is the step the refusal points at.
        1810 => invoke(world, "model.flatten"),
        1860 => tap(world, KeyCode::KeyI, "i"),
        1890 => type_word(world, "socket"),
        1920 => tap_named(world, KeyCode::Enter, Key::Enter),
        1980 => {
            let flash = world
                .resource::<crate::statusbar::StatusFlash>()
                .text
                .clone();
            check(
                world,
                flash.contains("Socket added"),
                "Socket authored through the add-component palette",
            );
            let gizmos = world
                .query_filtered::<(), With<crate::socket_gizmo::SocketGizmo>>()
                .iter(world)
                .count();
            check(world, gizmos > 0, "the authored socket shows its gizmo");
        }
        // ── The way back out: remove that component again, and undo it ─────
        1990 => invoke(world, "component.remove"),
        1993 => type_word(world, "socket"),
        1996 => tap_named(world, KeyCode::Enter, Key::Enter),
        1998 => {
            let sockets = world
                .query_filtered::<(), With<editor_prefabs::sockets::Socket>>()
                .iter(world)
                .count();
            check(world, sockets == 0, "the component was removed");
            invoke(world, "core.undo");
        }
        1999 => {
            let restored = world
                .query_filtered::<(), With<editor_prefabs::sockets::Socket>>()
                .iter(world)
                .count();
            check(world, restored > 0, "undo puts the removed component back");
        }
        // ── Author a real problem: enabled spinner with zero speed ─────────
        2000 => tap(world, KeyCode::KeyI, "i"),
        2030 => type_word(world, "spin"),
        2060 => tap_named(world, KeyCode::Enter, Key::Enter),
        2100 => {
            let field = world
                .query::<(Entity, &crate::inspector::InspectorField)>()
                .iter(world)
                .find(|(_, f)| f.path == "enabled")
                .map(|(e, _)| e);
            match field {
                Some(source) => world.trigger(bevy::ui_widgets::ValueChange {
                    source,
                    value: true,
                    is_final: true,
                }),
                None => check(world, false, "inspector 'enabled' field present"),
            }
        }
        2130 => {
            let field = world
                .query::<(Entity, &crate::inspector::InspectorField)>()
                .iter(world)
                .find(|(_, f)| f.path == "degrees_per_sec")
                .map(|(e, _)| e);
            match field {
                Some(source) => world.trigger(bevy::ui_widgets::ValueChange {
                    source,
                    value: 0.0f32,
                    is_final: true,
                }),
                None => check(world, false, "inspector 'degrees_per_sec' field present"),
            }
        }
        // ── The validator flags it; the problems panel jumps to it ─────────
        2170 => invoke(world, "level.validate"),
        2210 => {
            let warnings = world
                .resource::<editor_scene::level_validation::LevelValidation>()
                .count(editor_api::validate::Severity::Warning);
            check(
                world,
                warnings >= 1,
                "the game's level rule flagged the misconfigured spinner",
            );
            let opened = world.resource::<crate::problems::ProblemsState>().open;
            check(
                world,
                opened,
                "validate-with-problems auto-opened the panel",
            );
            shot(world, "24-level-problems");
        }
        2230 => {
            let row = world
                .query::<(Entity, &crate::problems::ProblemRow)>()
                .iter(world)
                .find(|(_, row)| row.0.is_some())
                .map(|(e, _)| e);
            match row.and_then(|r| ui_center(world, r)) {
                Some(center) => move_cursor(world, center),
                None => check(world, false, "an entity-shaped problem row exists"),
            }
        }
        2240 => click(world, true),
        2242 => click(world, false),
        2270 => {
            let offender = world
                .resource::<editor_scene::level_validation::LevelValidation>()
                .problems
                .iter()
                .find_map(|p| p.entity);
            let selected: Vec<SceneId> = world
                .query_filtered::<&SceneId, With<Selected>>()
                .iter(world)
                .copied()
                .collect();
            check(
                world,
                offender.is_some() && selected == vec![offender.unwrap()],
                "clicking the problem row selected the offender",
            );
        }
        2290 => tap_named(world, KeyCode::Escape, Key::Escape),
        2310 => tap_named(world, KeyCode::Escape, Key::Escape),
        2330 => {
            let closed = !world.resource::<crate::problems::ProblemsState>().open;
            check(world, closed, "empty-handed Escape closed the panel");
        }
        // ── Checklist: play the authored content, reset back ───────────────
        2350 => invoke(world, "editor.play"),
        2410 => {
            let playing = !world.resource::<EditorState>().active;
            check(world, playing, "F5 hands the authored world to the game");
        }
        2430 => invoke(world, "editor.reset"),
        2510 => {
            let back = world.resource::<EditorState>().active;
            check(world, back, "F7 returns to the editor");
            let drums = drum_roots(world).len();
            check(world, drums == 3, "authored instances survive play/reset");
        }
        // ── Lock: refuse edits, in batch, and say so ──────────────────────
        2620 => {
            let roots = drum_roots(world);
            // Through the kernel's own selection API, so the panels hear about
            // it the way they hear about a click.
            for (i, entity) in roots.iter().take(2).enumerate() {
                editor_core::selection::select_entity(world, *entity, i > 0);
            }
            let held: Vec<Vec3> = roots
                .iter()
                .take(2)
                .map(|e| world.get::<Transform>(*e).unwrap().translation)
                .collect();
            world.resource_mut::<HandsonProbe>().locked_before = held;
        }
        2630 => tap_named(world, KeyCode::Space, bevy::input::keyboard::Key::Space),
        2636 => tap(world, KeyCode::KeyL, "l"),
        2660 => {
            let roots = drum_roots(world);
            let locked = roots
                .iter()
                .take(2)
                .all(|e| world.get::<editor_core::lock::Locked>(*e).is_some());
            check(world, locked, "Space l locks the WHOLE selection at once");
            let third = roots.get(2).copied();
            check(
                world,
                third.is_some_and(|e| world.get::<editor_core::lock::Locked>(e).is_none()),
                "locking the selection leaves the rest of the level alone",
            );
        }
        // The real delete verb, refused at the one choke point.
        2670 => invoke(world, "select.delete"),
        2700 => {
            let roots = drum_roots(world);
            let spoke = world
                .resource::<crate::statusbar::StatusFlash>()
                .text
                .contains("locked");
            check(
                world,
                roots.len() == 3,
                "a locked object refuses the delete verb",
            );
            // Refusing SILENTLY is the real failure here: an editor where the
            // delete key does nothing and says nothing reads as broken.
            check(world, spoke, "the refusal says why, in the statusbar");
            let before = world.resource::<HandsonProbe>().locked_before.clone();
            let now: Vec<Vec3> = roots
                .iter()
                .take(2)
                .map(|e| world.get::<Transform>(*e).unwrap().translation)
                .collect();
            check(world, before == now, "locked objects did not move");
        }
        // Unlocking is the one edit a locked object must accept.
        // The leader sequence, tapped as a sequence.
        2710 => tap_named(world, KeyCode::Space, bevy::input::keyboard::Key::Space),
        2716 => tap(world, KeyCode::KeyL, "l"),
        2740 => {
            let roots = drum_roots(world);
            let free = roots
                .iter()
                .take(2)
                .all(|e| world.get::<editor_core::lock::Locked>(*e).is_none());
            check(world, free, "Space l again unlocks the selection");
        }
        // ...and then it deletes like anything else.
        2750 => invoke(world, "select.delete"),
        2790 => {
            let remaining = drum_roots(world).len();

            check(world, remaining == 1, "an unlocked object deletes normally");
            world.resource_mut::<HistoryRequests>().undo = 1;
        }
        2830 => {
            let restored = drum_roots(world).len();
            check(
                world,
                restored == 3,
                "undo brings the deleted instances back",
            );
        }
        // ── Batch edit: one field, the whole selection ────────────────────
        2840 => {
            let roots = drum_roots(world);
            // Through the kernel's own selection API, so the panels hear about
            // it the way they hear about a click.
            for (i, entity) in roots.iter().take(2).enumerate() {
                editor_core::selection::select_entity(world, *entity, i > 0);
            }
            world.resource_mut::<HandsonProbe>().locked_before = roots
                .iter()
                .take(2)
                .map(|e| world.get::<Transform>(*e).unwrap().translation)
                .collect();
        }
        // The REAL inspector row, which shows the first selected object only.
        2880 => {
            let field = world
                .query::<(Entity, &crate::inspector::InspectorField)>()
                .iter(world)
                .find(|(_, f)| f.path == "translation.y")
                .map(|(entity, _)| entity);
            match field {
                Some(source) => world.trigger(bevy::ui_widgets::ValueChange {
                    source,
                    value: 4.0f32,
                    is_final: true,
                }),
                None => check(world, false, "inspector Y field present for the batch"),
            }
        }
        2920 => {
            let roots = drum_roots(world);
            let before = world.resource::<HandsonProbe>().locked_before.clone();
            let now: Vec<Vec3> = roots
                .iter()
                .take(2)
                .map(|e| world.get::<Transform>(*e).unwrap().translation)
                .collect();
            check(
                world,
                now.len() == 2 && now.iter().all(|p| (p.y - 4.0).abs() < 1e-4),
                "one inspector field edit reached BOTH selected objects",
            );
            // The trap: the shown object's whole component going to everyone.
            check(
                world,
                before.len() == 2
                    && before[0].x == now[0].x
                    && before[1].x == now[1].x
                    && before[0].x != before[1].x,
                "a batch edit left every OTHER field of each object alone",
            );
            let third_moved = roots.get(2).is_some_and(|e| {
                world
                    .get::<Transform>(*e)
                    .map(|t| t.translation.y)
                    .unwrap_or(0.0)
                    > 3.9
            });
            check(world, !third_moved, "the batch stopped at the selection");
        }
        // ── Cleanup: nothing probe-owned outlives the run ──────────────────
        2960 => {
            if let Some(id) = world.resource::<HandsonProbe>().material {
                let mut library = world.resource_mut::<MaterialLibrary>();
                library.materials.retain(|d| d.id != id);
                library.generation += 1;
            }
        }
        // ── `*` on a plain object: the GAME's own kind rung ───────────────
        // No fixture is built. The level already ships four "Box" roots, all
        // `Primitive { kind: Cube }` — a probe that placed its own cubes and
        // then asserted "exactly the cubes I placed" would have been asserting
        // something untrue about the level it was running in.
        3010 => invoke(world, "select.clear"),
        3020 => {
            if let Some(first) = named_roots(world, "Box").first().copied() {
                editor_core::selection::select_entity(world, first, false);
            }
        }
        // `*` is physically shift+8, and the shift has to land in an EARLIER
        // frame: injected keys only reach `ButtonInput` on the next frame, so a
        // same-frame shift leaves the resolver seeing a bare 8 — which is the
        // count prefix, not a verb.
        3026 => key(world, KeyCode::ShiftLeft, Key::Shift, None, true),
        3030 => tap(world, KeyCode::Digit8, "8"),
        3034 => key(world, KeyCode::ShiftLeft, Key::Shift, None, false),
        3070 => {
            let boxes = named_roots(world, "Box");
            let selected = selected_entities(world);
            check(
                world,
                boxes.len() > 1 && boxes.iter().all(|e| selected.contains(e)),
                &format!(
                    "* selected every object of the same kind ({} of {})",
                    selected.len(),
                    boxes.len()
                ),
            );
            let crossed = drum_roots(world).iter().any(|e| selected.contains(e));
            check(world, !crossed, "* did not cross into another rung");
            let flash = flash_text(world);
            check(
                world,
                flash.contains("same primitive"),
                &format!("* names the family it matched [{flash}]"),
            );
        }
        // ── `*` on a prefab instance: the PREFAB rung and the seal rule ───
        3090 => {
            if let Some(first) = drum_roots(world).first().copied() {
                editor_core::selection::select_entity(world, first, false);
            }
        }
        3096 => key(world, KeyCode::ShiftLeft, Key::Shift, None, true),
        3100 => tap(world, KeyCode::Digit8, "8"),
        3104 => key(world, KeyCode::ShiftLeft, Key::Shift, None, false),
        3140 => {
            let drums = drum_roots(world);
            let selected = selected_entities(world);
            check(
                world,
                drums.iter().all(|e| selected.contains(e)) && selected.len() == drums.len(),
                &format!("* selected exactly the drum instances ({})", selected.len()),
            );
            // The likely bug is copying select.all's flat With<SceneId> sweep,
            // which returns prefab MEMBERS unlike a click or a box.
            let members = world
                .query_filtered::<Entity, With<PrefabStamped>>()
                .iter(world)
                .filter(|e| selected.contains(e))
                .count();
            check(world, members == 0, "* stopped at the prefab seal");
            let boxes_left = named_roots(world, "Box")
                .iter()
                .filter(|e| selected.contains(e))
                .count();
            check(world, boxes_left == 0, "* replaced the previous selection");
        }
        // ── space h ───────────────────────────────────────────────────────
        3160 => {
            if let Some(first) = drum_roots(world).first().copied() {
                editor_core::selection::select_entity(world, first, false);
            }
            let depth = world.resource::<History>().undo_depth();
            let dirty = world.resource::<editor_scene::SceneDirty>().0;
            world.resource_mut::<HandsonProbe>().before_hide = Some((depth, dirty));
        }
        3170 => tap_named(world, KeyCode::Space, Key::Space),
        3176 => tap(world, KeyCode::KeyH, "h"),
        3210 => {
            let drum = drum_roots(world).first().copied();
            // NOT the root: a prefab instance root carries no Mesh3d at all, so
            // asserting on it would pass with nothing hidden.
            let dark = drum.is_some_and(|root| mesh_descendant_visible(world, root) == Some(false));
            check(world, dark, "space h took the object out of the view");
            check(
                world,
                drum.is_some(),
                "the hidden object is still in the level",
            );
            check(
                world,
                drum.is_some_and(|e| world.get::<Selected>(e).is_none()),
                "hide deselects, so the next verb cannot land blind",
            );
            let (depth, dirty) = world
                .resource::<HandsonProbe>()
                .before_hide
                .unwrap_or_default();
            check(
                world,
                world.resource::<History>().undo_depth() == depth,
                "hide spent no undo step — it is a view, not an edit",
            );
            check(
                world,
                world.resource::<editor_scene::SceneDirty>().0 == dirty,
                "hide did not dirty the scene",
            );
            let flash = flash_text(world);
            check(
                world,
                flash.contains("\u{2423}u"),
                &format!("hide names its own way back [{flash}]"),
            );
        }
        // ── the hidden-but-selected outline leak ──────────────────────────
        3220 => {
            if let Some(drum) = drum_roots(world).first().copied() {
                editor_core::selection::select_entity(world, drum, false);
            }
        }
        3250 => {
            // The JFA silhouette is an unparented root at the source's world
            // transform, so Visibility on the object never reaches it.
            let drum = drum_roots(world).first().copied();
            let outlined = drum.is_some_and(|root| subtree_is_outlined(world, root));
            check(
                world,
                !outlined,
                "a hidden object draws no outline around empty space",
            );
            invoke(world, "select.clear");
        }
        // ── a hidden object does not eat the click ────────────────────────
        3270 => {
            if let Some(drum) = drum_roots(world).first().copied()
                && let Some(at) = screen_of(world, drum)
            {
                move_cursor(world, at);
            }
        }
        3276 => click(world, true),
        3278 => click(world, false),
        3300 => {
            let drum = drum_roots(world).first().copied();
            check(
                world,
                drum.is_some_and(|e| world.get::<Selected>(e).is_none()),
                "hide the roof and the click reaches the floor",
            );
        }
        // ── `*` skips hidden ──────────────────────────────────────────────
        3320 => {
            if let Some(last) = named_roots(world, "Box").last().copied() {
                editor_core::selection::select_entity(world, last, false);
            }
        }
        3326 => tap_named(world, KeyCode::Space, Key::Space),
        3332 => tap(world, KeyCode::KeyH, "h"),
        3360 => {
            if let Some(first) = named_roots(world, "Box").first().copied() {
                editor_core::selection::select_entity(world, first, false);
            }
        }
        3366 => key(world, KeyCode::ShiftLeft, Key::Shift, None, true),
        3370 => tap(world, KeyCode::Digit8, "8"),
        3374 => key(world, KeyCode::ShiftLeft, Key::Shift, None, false),
        3410 => {
            let boxes = named_roots(world, "Box");
            let selected = selected_entities(world);
            check(
                world,
                selected.len() == boxes.len() - 1,
                &format!(
                    "* skipped the hidden one ({} of {})",
                    selected.len(),
                    boxes.len()
                ),
            );
            let flash = flash_text(world);
            check(
                world,
                flash.contains("hidden skipped"),
                &format!("* said what it left out [{flash}]"),
            );
        }
        // ── isolate, and that it RESTORES rather than reveals ─────────────
        3430 => {
            if let Some(first) = named_roots(world, "Box").first().copied() {
                editor_core::selection::select_entity(world, first, false);
            }
        }
        3436 => tap_named(world, KeyCode::Space, Key::Space),
        3442 => key(world, KeyCode::ShiftLeft, Key::Shift, None, true),
        3446 => tap(world, KeyCode::KeyH, "h"),
        3450 => key(world, KeyCode::ShiftLeft, Key::Shift, None, false),
        3490 => {
            let boxes = named_roots(world, "Box");
            let focus_lit = boxes
                .first()
                .is_some_and(|e| mesh_descendant_visible(world, *e) != Some(false));
            check(world, focus_lit, "isolate left its own focus lit");
            let neighbour_dark = boxes
                .get(1)
                .is_some_and(|e| mesh_descendant_visible(world, *e) == Some(false));
            check(world, neighbour_dark, "isolate hid everything else");
            check(
                world,
                world.resource::<editor_core::hide::Hidden>().is_isolated(),
                "isolate is a state you can leave",
            );
        }
        3510 => tap_named(world, KeyCode::Space, Key::Space),
        3516 => key(world, KeyCode::ShiftLeft, Key::Shift, None, true),
        3520 => tap(world, KeyCode::KeyH, "h"),
        3524 => key(world, KeyCode::ShiftLeft, Key::Shift, None, false),
        3560 => {
            // THE sharp one: a naive "isolate exits by unhiding everything"
            // passes every other arm here and fails only this.
            let drum_still_hidden = drum_roots(world)
                .first()
                .is_some_and(|e| mesh_descendant_visible(world, *e) == Some(false));
            let box_still_hidden = named_roots(world, "Box")
                .last()
                .is_some_and(|e| mesh_descendant_visible(world, *e) == Some(false));
            check(
                world,
                drum_still_hidden && box_still_hidden,
                "leaving isolate RESTORED what was hidden before it",
            );
            let neighbour_back = named_roots(world, "Box")
                .get(1)
                .is_some_and(|e| mesh_descendant_visible(world, *e) != Some(false));
            check(
                world,
                neighbour_back,
                "leaving isolate brought the rest back",
            );
            check(
                world,
                !world.resource::<editor_core::hide::Hidden>().is_isolated(),
                "isolate is off again",
            );
        }
        // ── space u ───────────────────────────────────────────────────────
        3580 => tap_named(world, KeyCode::Space, Key::Space),
        3586 => tap(world, KeyCode::KeyU, "u"),
        3620 => {
            check(
                world,
                world.resource::<editor_core::hide::Hidden>().is_empty(),
                "space u brought everything back",
            );
        }
        // ── play lifts hide; reset brings it back through the respawn ─────
        3640 => {
            if let Some(drum) = drum_roots(world).first().copied() {
                editor_core::selection::select_entity(world, drum, false);
            }
        }
        3646 => tap_named(world, KeyCode::Space, Key::Space),
        3652 => tap(world, KeyCode::KeyH, "h"),
        3680 => {
            let dark = drum_roots(world)
                .first()
                .is_some_and(|e| mesh_descendant_visible(world, *e) == Some(false));
            check(world, dark, "hide is on before play");
            invoke(world, "editor.play");
        }
        3720 => {
            // F5 shows the REAL level. Nobody playtests against a level that is
            // secretly missing its floor.
            let lit = drum_roots(world)
                .first()
                .is_some_and(|e| mesh_descendant_visible(world, *e) != Some(false));
            check(
                world,
                !world.resource::<EditorState>().active && lit,
                "play lifts hide",
            );
        }
        3730 => invoke(world, "editor.reset"),
        3770 => {
            // Same SceneId, brand-new Entity: this is what keying by SceneId buys.
            let dark = drum_roots(world)
                .first()
                .is_some_and(|e| mesh_descendant_visible(world, *e) == Some(false));
            check(world, dark, "hide survives the play/reset respawn");
        }
        // Nothing probe-owned outlives the run.
        3780 => tap_named(world, KeyCode::Space, Key::Space),
        3786 => tap(world, KeyCode::KeyU, "u"),
        // ── ARRAY: a run of copies, spaced by the piece's own width ───────
        3820 => invoke(world, "select.clear"),
        3830 => {
            let roots = drum_roots(world);
            if let Some(first) = roots.first().copied() {
                editor_core::selection::select_entity(world, first, false);
                let pose = *world.get::<Transform>(first).unwrap();
                let depth = world.resource::<History>().undo_depth();
                world.resource_mut::<HandsonProbe>().array_before =
                    Some((depth, roots.len(), pose));
                world.resource_mut::<HandsonProbe>().array_roots_before = roots.clone();
            }
        }
        // The gate: without it every assertion below is vacuous.
        3840 => {
            let selected = selected_entities(world);
            check(
                world,
                selected.len() == 1,
                &format!("one drum selected to array ({})", selected.len()),
            );
        }
        3850 => tap_named(world, KeyCode::Space, Key::Space),
        3856 => tap(world, KeyCode::KeyX, "x"),
        3862 => tap(world, KeyCode::KeyX, "x"),
        3890 => {
            let open = world
                .resource::<editor_prefabs::authoring::GroupPrompt>()
                .open;
            check(world, open, "space x x asks for a count");
        }
        3900 => tap(world, KeyCode::Digit3, "3"),
        3910 => tap_named(world, KeyCode::Enter, Key::Enter),
        3990 => {
            let (depth, before, pose) = world.resource::<HandsonProbe>().array_before.unwrap_or((
                0,
                0,
                Transform::IDENTITY,
            ));
            let roots = drum_roots(world);
            check(
                world,
                roots.len() == before + 3,
                &format!(
                    "array laid exactly 3 copies ({} -> {})",
                    before,
                    roots.len()
                ),
            );
            // The COPIES, identified by difference — the level's own drums have
            // been moved around by earlier segments and are not part of this run.
            let before_set = world.resource::<HandsonProbe>().array_roots_before.clone();
            let mut deltas: Vec<f32> = roots
                .iter()
                .filter(|e| !before_set.contains(e))
                .filter_map(|e| world.get::<Transform>(*e).map(|t| t.translation.x))
                .map(|x| x - pose.translation.x)
                .collect();
            deltas.sort_by(f32::total_cmp);
            let step = deltas.first().copied().unwrap_or(0.0);
            let stepped = deltas.len() == 3
                && step > 0.1
                && (1..=3).all(|k| deltas.iter().any(|d| (d - step * k as f32).abs() < 1e-3));
            check(
                world,
                stepped,
                &format!(
                    "the copies step by the piece's own width ({step:.3}m, offsets {deltas:?})"
                ),
            );
            // Each copy is a real instance of THAT prefab, and visible — an
            // array of husks would satisfy a count-only assertion.
            let source_prefab = roots
                .first()
                .and_then(|e| world.get::<editor_prefabs::PrefabInstance>(*e))
                .map(|i| i.0);
            let all_real = roots.iter().all(|e| {
                world.get::<editor_prefabs::PrefabInstance>(*e).map(|i| i.0) == source_prefab
            });
            check(
                world,
                all_real,
                "every copy is an instance of the same prefab",
            );
            let all_drawn = roots
                .iter()
                .all(|e| mesh_descendant_visible(world, *e) == Some(true));
            check(world, all_drawn, "every copy actually has geometry");
            let source_put = roots.first().is_some_and(|e| {
                world.get::<Transform>(*e).unwrap().translation == pose.translation
            });
            check(world, source_put, "array did not move its own source");
            check(
                world,
                world.resource::<History>().undo_depth() == depth + 1,
                "the whole run is ONE undo entry",
            );
            let flash = flash_text(world);
            check(
                world,
                flash.starts_with("arrayed 3"),
                &format!("array says what it laid [{flash}]"),
            );
        }
        4010 => invoke(world, "core.undo"),
        4060 => {
            let (_, before, pose) = world.resource::<HandsonProbe>().array_before.unwrap_or((
                0,
                0,
                Transform::IDENTITY,
            ));
            let roots = drum_roots(world);
            check(
                world,
                roots.len() == before,
                &format!("one undo removes the whole run ({})", roots.len()),
            );
            check(
                world,
                roots.first().is_some_and(|e| {
                    world.get::<Transform>(*e).unwrap().translation == pose.translation
                }),
                "the source survived the undo",
            );
        }
        // ── MIRROR: two drums swap across their own centre ───────────────
        4080 => {
            let roots = drum_roots(world);
            if roots.len() >= 2 {
                editor_core::selection::select_entity(world, roots[0], false);
                editor_core::selection::select_entity(world, roots[1], true);
                let poses = roots
                    .iter()
                    .take(2)
                    .map(|e| (*e, *world.get::<Transform>(*e).unwrap()))
                    .collect();
                world.resource_mut::<HandsonProbe>().mirror_before = poses;
            }
        }
        4090 => {
            let selected = selected_entities(world);
            check(
                world,
                selected.len() == 2,
                &format!("two drums selected to mirror ({})", selected.len()),
            );
        }
        4100 => tap_named(world, KeyCode::Space, Key::Space),
        4106 => tap(world, KeyCode::KeyX, "x"),
        // Shift MUST be held across frames: injected keys reach ButtonInput
        // only on the next frame, so a same-frame press+release never registers.
        4110 => key(world, KeyCode::ShiftLeft, Key::Shift, None, true),
        4114 => tap(world, KeyCode::KeyX, "x"),
        4118 => key(world, KeyCode::ShiftLeft, Key::Shift, None, false),
        4160 => {
            let before = world.resource::<HandsonProbe>().mirror_before.clone();
            let mid = before.iter().map(|(_, t)| t.translation.x).sum::<f32>()
                / before.len().max(1) as f32;
            let mut reflected = true;
            let mut moved = true;
            for (entity, was) in &before {
                let Some(now) = world.get::<Transform>(*entity).map(|t| t.translation.x) else {
                    reflected = false;
                    continue;
                };
                if ((2.0 * mid - was.translation.x) - now).abs() > 1e-3 {
                    reflected = false;
                }
                // Anti-vacuity: a no-op satisfies the reflection when a piece
                // already sits on the plane, so demand real movement too.
                if (now - was.translation.x).abs() < 0.5 {
                    moved = false;
                }
            }
            check(
                world,
                reflected,
                "each drum reflected across the pair's centre",
            );
            check(world, moved, "the mirror actually moved them");
            // THE invariant: no negative scale ever reaches the level.
            let positive = before.iter().all(|(entity, _)| {
                world
                    .get::<Transform>(*entity)
                    .is_some_and(|t| t.scale.x > 0.0 && t.scale.y > 0.0 && t.scale.z > 0.0)
            });
            check(world, positive, "no negative scale reached the level");
            let flash = flash_text(world);
            check(
                world,
                flash.contains("geometry is not flipped"),
                &format!("mirror tells the truth about what it did [{flash}]"),
            );
        }
        4180 => invoke(world, "core.undo"),
        4230 => {
            let before = world.resource::<HandsonProbe>().mirror_before.clone();
            let restored = before.iter().all(|(entity, was)| {
                world
                    .get::<Transform>(*entity)
                    .is_some_and(|t| (t.translation - was.translation).length() < 1e-4)
            });
            check(world, restored, "one undo restores both poses exactly");
        }
        // ── A locked object refuses both verbs, and says which ───────────
        4240 => {
            if let Some(first) = drum_roots(world).first().copied() {
                editor_core::selection::select_entity(world, first, false);
            }
        }
        4246 => tap_named(world, KeyCode::Space, Key::Space),
        4252 => tap(world, KeyCode::KeyL, "l"),
        4270 => tap_named(world, KeyCode::Space, Key::Space),
        4276 => tap(world, KeyCode::KeyX, "x"),
        4280 => tap(world, KeyCode::KeyX, "x"),
        // The count has to be TYPED, or perform_array never runs and the
        // assertion below reads a flash left over from the lock itself.
        4286 => tap(world, KeyCode::Digit2, "2"),
        4292 => tap_named(world, KeyCode::Enter, Key::Enter),
        4340 => {
            let roots = drum_roots(world).len();
            let flash = flash_text(world);
            check(
                world,
                flash.contains("locked") && flash.contains("\u{2423}l"),
                &format!("array refuses a locked source out loud [{flash}]"),
            );
            world.resource_mut::<HandsonProbe>().array_before =
                Some((0, roots, Transform::IDENTITY));
        }
        4350 => {
            let (_, after_refusal, _) = world.resource::<HandsonProbe>().array_before.unwrap_or((
                0,
                0,
                Transform::IDENTITY,
            ));
            let now = drum_roots(world).len();
            check(world, now == after_refusal, "a refused array lays nothing");
        }
        4360 => tap_named(world, KeyCode::Space, Key::Space),
        4366 => tap(world, KeyCode::KeyL, "l"),
        // ── Deleting a group and undoing it restores the WHOLE subtree ────
        // The subject is the flattened barrel: `model.flatten` (frame 1810)
        // turns an import into real `MeshNode` children with their own
        // `SceneId`s, which is the only genuine parent/child scene content the
        // level has. A prefab instance would not do — its members are DERIVED
        // and deliberately not captured.
        4410 => invoke(world, "select.clear"),
        4420 => {
            let subject = scene_root_with_scene_children(world);
            match subject {
                Some((root, children)) => {
                    editor_core::selection::select_entity(world, root, false);
                    let mut ids = vec![*world.get::<SceneId>(root).unwrap()];
                    ids.extend(
                        children
                            .iter()
                            .filter_map(|c| world.get::<SceneId>(*c).copied()),
                    );
                    world.resource_mut::<HandsonProbe>().delete_subject =
                        Some((root, children.clone()));
                    world.resource_mut::<HandsonProbe>().delete_ids = ids;
                    check(
                        world,
                        children.len() >= 2,
                        &format!("a real parent with children to delete ({})", children.len()),
                    );
                }
                None => check(world, false, "a real parent with children to delete"),
            }
        }
        4430 => {
            let selected = selected_entities(world).len();
            check(
                world,
                selected == 1,
                &format!("the parent is selected ({selected})"),
            );
        }
        4436 => tap(world, KeyCode::KeyD, "d"),
        4470 => {
            let (root, children) = world
                .resource::<HandsonProbe>()
                .delete_subject
                .clone()
                .unwrap_or((Entity::PLACEHOLDER, Vec::new()));
            let gone = world.get_entity(root).is_err()
                && children.iter().all(|c| world.get_entity(*c).is_err());
            check(world, gone, "d took the parent and its children");
            // Record the ids, because undo hands back NEW entities for the same
            // scene ids — an Entity-keyed assertion would fail for the wrong
            // reason.
            let _ = root;
        }
        4480 => invoke(world, "core.undo"),
        4530 => {
            let ids = world.resource::<HandsonProbe>().delete_ids.clone();
            let index = world.resource::<SceneIndex>();
            let back: Vec<Option<Entity>> = ids.iter().map(|id| index.get(id)).collect();
            let all_back = back.iter().all(|e| e.is_some());
            check(
                world,
                all_back,
                &format!(
                    "undo brought the whole subtree back ({}/{})",
                    back.iter().filter(|e| e.is_some()).count(),
                    ids.len()
                ),
            );
            // And the SHAPE, not just the entities: this is the half that used
            // to come back silently wrong.
            let root_id = ids.first().copied();
            let reparented = ids.iter().skip(1).all(|id| {
                world
                    .resource::<SceneIndex>()
                    .get(id)
                    .and_then(|e| world.get::<ChildOf>(e))
                    .map(|c| c.parent())
                    .and_then(|p| world.get::<SceneId>(p).copied())
                    == root_id
            });
            check(
                world,
                reparented,
                "every child came back under its own parent",
            );
        }
        // ── A parent and its child, selected together, move ONCE ──────────
        // The subject is the flattened barrel again — the only real
        // parent/child scene content the level has. `select.all` is invoked as
        // data rather than tapped: the binding is covered elsewhere, and this
        // removes every modifier-timing risk from the assertion that matters.
        4540 => {
            let subject = scene_root_with_scene_children(world);
            match subject {
                Some((root, children)) => {
                    let poses = std::iter::once(root)
                        .chain(children.iter().copied())
                        .filter_map(|e| world.get::<Transform>(e).map(|t| (e, *t)))
                        .collect::<Vec<_>>();
                    world.resource_mut::<HandsonProbe>().move_before = poses;
                }
                None => check(world, false, "a real parent with children to move"),
            }
        }
        4546 => invoke(world, "select.all"),
        4552 => {
            let before = world.resource::<HandsonProbe>().move_before.clone();
            let selected = selected_entities(world);
            // The gate: select.all must actually have taken the children, or
            // the fold has nothing to fold and every assertion below is vacuous.
            let took_children = before
                .iter()
                .skip(1)
                .all(|(entity, _)| selected.contains(entity));
            check(
                world,
                took_children && before.len() >= 3,
                &format!(
                    "select.all took the parent AND its children ({} rows)",
                    before.len()
                ),
            );
        }
        4560 => invoke(world, "transform.move"),
        4566 => invoke(world, "transform.axis-x"),
        4572 => invoke(world, "transform.digit-2"),
        4578 => invoke(world, "transform.commit"),
        4596 => {
            let before = world.resource::<HandsonProbe>().move_before.clone();
            let mut parent_moved = false;
            let mut children_still = true;
            for (index, (entity, was)) in before.iter().enumerate() {
                let Some(now) = world.get::<Transform>(*entity).copied() else {
                    children_still = false;
                    continue;
                };
                if index == 0 {
                    parent_moved = (now.translation.x - was.translation.x - 2.0).abs() < 1e-3;
                } else if (now.translation - was.translation).length() > 1e-4 {
                    // A carried child's LOCAL transform must come out
                    // bit-identical: it rides its parent, it does not also move
                    // itself. Before the fold each child gained the delta too,
                    // so its world position moved twice as far.
                    children_still = false;
                }
            }
            check(
                world,
                parent_moved,
                "the parent moved by exactly the typed amount",
            );
            check(
                world,
                children_still,
                "its children rode along instead of moving themselves as well",
            );
        }
        4600 => {
            let failures = world.resource::<HandsonProbe>().failures.clone();
            if failures.is_empty() {
                info!("HANDSON-PROBE PASS: the owner hands-on checklist end-to-end");
                world.write_message(AppExit::Success);
            } else {
                error!("HANDSON-PROBE FAILED: {failures:?}");
                world.write_message(AppExit::error());
            }
        }
        _ => {}
    }
}
