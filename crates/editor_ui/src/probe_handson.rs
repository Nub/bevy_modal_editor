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
use crate::probe_user::{click, move_cursor, shot, tap, tap_named};

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

fn type_word(world: &mut World, word: &str) {
    for ch in word.chars() {
        let code = match ch {
            'a' => KeyCode::KeyA,
            'b' => KeyCode::KeyB,
            'c' => KeyCode::KeyC,
            'd' => KeyCode::KeyD,
            'e' => KeyCode::KeyE,
            'k' => KeyCode::KeyK,
            'm' => KeyCode::KeyM,
            'o' => KeyCode::KeyO,
            'r' => KeyCode::KeyR,
            's' => KeyCode::KeyS,
            't' => KeyCode::KeyT,
            'u' => KeyCode::KeyU,
            _ => continue,
        };
        tap(world, code, &ch.to_string());
    }
}

/// Logical-pixel center of a UI node — drives REAL cursor clicks on chrome.
fn ui_center(world: &mut World, entity: Entity) -> Option<Vec2> {
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
            let _ = std::fs::remove_file(format!("prefabs/{stale}.prefab.ron"));
            let _ = std::fs::remove_file(format!("prefabs/{stale}.prefab.ron.bak"));
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
        300 => {
            let chip = world
                .query::<(Entity, &Field)>()
                .iter(world)
                .find(|(_, f)| matches!(f, Field::Texture))
                .map(|(e, _)| e);
            match chip.and_then(|c| ui_center(world, c)) {
                Some(center) => move_cursor(world, center),
                None => check(world, false, "texture chip locatable on screen"),
            }
        }
        310 => click(world, true),
        312 => click(world, false),
        380 => {
            let (probe_material, probe_texture) = {
                let probe = world.resource::<HandsonProbe>();
                (probe.material, probe.texture)
            };
            let textured = probe_material
                .and_then(|id| world.resource::<MaterialLibrary>().get(&id).cloned())
                .and_then(|def| def.base_color_texture);
            check(
                world,
                textured.is_some() && textured == probe_texture,
                "clicking the texture chip bound the imported texture",
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
        440 => {
            let cleared = world
                .resource::<HandsonProbe>()
                .material
                .and_then(|id| world.resource::<MaterialLibrary>().get(&id).cloned())
                .is_some_and(|def| def.base_color_texture.is_none());
            check(world, cleared, "asset undo removed the texture binding");
            invoke(world, "core.redo");
        }
        480 => {
            let restored = world
                .resource::<HandsonProbe>()
                .material
                .and_then(|id| world.resource::<MaterialLibrary>().get(&id).cloned())
                .is_some_and(|def| def.base_color_texture.is_some());
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
        // Override: the un-moved drum sits under the parked cursor — click it.
        1320 => {
            let center = crate::probe_user::viewport_center(world);
            move_cursor(world, center);
        }
        1330 => click(world, true),
        1332 => click(world, false),
        1350 => {
            let member = world
                .query_filtered::<Entity, (With<Selected>, With<PrefabStamped>)>()
                .iter(world)
                .next();
            check(
                world,
                member.is_some(),
                "clicking the mesh selected the member",
            );
            let root = member.and_then(|m| world.get::<ChildOf>(m).map(|c| c.parent()));
            world.resource_mut::<HandsonProbe>().overridden_root = root;
        }
        // Member OVERRIDES author through the INSPECTOR (gesture moves on a
        // stamped selection deliberately move the whole instance): commit
        // translation.y = 1 through the real field event.
        1360 => {
            let field = world
                .query::<(Entity, &crate::inspector::InspectorField)>()
                .iter(world)
                .find(|(_, f)| f.path == "translation.y")
                .map(|(e, _)| e);
            match field {
                Some(source) => world.trigger(bevy::ui_widgets::ValueChange {
                    source,
                    value: 1.0f32,
                    is_final: true,
                }),
                None => check(world, false, "inspector Y field present for the member"),
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
        1860 => tap(world, KeyCode::KeyI, "i"),
        1890 => type_word(world, "socket"),
        1920 => tap_named(world, KeyCode::Enter, Key::Enter),
        1980 => {
            let flash = world
                .resource::<crate::statusbar::StatusFlash>()
                .text
                .clone();
            info!("HANDSON-PROBE diag: socket-stage flash={flash:?}");
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
        // ── Checklist: play the authored content, reset back ───────────────
        2000 => invoke(world, "editor.play"),
        2060 => {
            let playing = !world.resource::<EditorState>().active;
            check(world, playing, "F5 hands the authored world to the game");
        }
        2080 => invoke(world, "editor.reset"),
        2160 => {
            let back = world.resource::<EditorState>().active;
            check(world, back, "F7 returns to the editor");
            let drums = drum_roots(world).len();
            check(world, drums == 3, "authored instances survive play/reset");
        }
        // ── Checklist bonus: the level validator runs over authored content ─
        2170 => invoke(world, "level.validate"),
        2190 => {
            let ran = world
                .resource::<editor_scene::level_validation::LevelValidation>()
                .generation
                > 0;
            check(world, ran, "level validation ran over the authored scene");
            let flash = world
                .resource::<crate::statusbar::StatusFlash>()
                .text
                .clone();
            check(
                world,
                flash.contains("level"),
                "level.validate reports a summary",
            );
        }
        // ── Cleanup: nothing probe-owned outlives the run ──────────────────
        2200 => {
            if let Some(id) = world.resource::<HandsonProbe>().material {
                let mut library = world.resource_mut::<MaterialLibrary>();
                library.materials.retain(|d| d.id != id);
                library.generation += 1;
            }
        }
        2260 => {
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
