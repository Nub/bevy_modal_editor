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

/// Where an entity actually IS on screen. Clicking a fixed fraction of the
/// window only worked while the drums happened to project over that spot: it
/// missed by a few pixels the moment frame timing shifted the placement, which
/// made an unrelated change look like a selection bug.
fn screen_position_of(world: &mut World, target: Vec3) -> Option<Vec2> {
    let (camera, camera_transform) = world
        .query_filtered::<(&Camera, &GlobalTransform), With<Camera3d>>()
        .iter(world)
        // The material preview rig has its own active camera rendering to a
        // texture; only the one drawing the VIEWPORT can be projected through.
        .max_by_key(|(camera, _)| camera.order)
        .map(|(camera, transform)| (camera.clone(), *transform))?;
    camera.world_to_viewport(&camera_transform, target).ok()
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
        // ── Cleanup: nothing probe-owned outlives the run ──────────────────
        2550 => {
            if let Some(id) = world.resource::<HandsonProbe>().material {
                let mut library = world.resource_mut::<MaterialLibrary>();
                library.materials.retain(|d| d.id != id);
                library.generation += 1;
            }
        }
        2610 => {
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
