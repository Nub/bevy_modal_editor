//! THE BARREL WORKFLOW probe (BARREL_PROBE=1, M4-D12, spec §6 — the milestone
//! exit): drive the real session through drop-GLB → import/validate → place →
//! prefab → instances → artist re-export → re-import → every instance updates
//! with transforms intact. The GLB is generated from `editor_assets::fixture`
//! so the fixture never rots; scale 1.0 vs 2.5 models the re-export.

use bevy::input::keyboard::Key;
use bevy::mesh::VertexAttributeValues;
use bevy::prelude::*;
use editor_core::prelude::*;
use editor_prefabs::{PrefabInstance, PrefabLibrary};
use editor_scene::PrefabStamped;
use editor_scene::models::{MeshRef, MeshRefDerived, ModelLibrary, ProcessedAssets};
use uuid::Uuid;

use crate::probe_user::{shot, tap, tap_named};

#[derive(Resource, Default)]
pub(crate) struct BarrelProbe {
    frame: u32,
    failures: Vec<String>,
    /// Identity captured at first import — must survive the re-export.
    uuid: Option<Uuid>,
    /// Instance-root positions captured before the re-import.
    positions: Vec<(Entity, Vec3)>,
    /// Size the Process stage recorded at first import.
    bounds: Option<[f32; 3]>,
}

fn check(world: &mut World, ok: bool, what: &str) {
    if ok {
        info!("BARREL-PROBE PASS: {what}");
    } else {
        error!("BARREL-PROBE FAIL: {what}");
        world
            .resource_mut::<BarrelProbe>()
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

fn glb_path(world: &World) -> std::path::PathBuf {
    world
        .resource::<ModelLibrary>()
        .fs_root
        .join(editor_scene::models::MODELS_DIR)
        .join("barrel.glb")
}

/// Max Y over every vertex of every mesh spawned under a MeshRef subtree —
/// scale 1.0 barrels top out at ~1.0, re-exported 2.5 ones at ~2.5.
fn barrel_mesh_tops(world: &mut World) -> Vec<f32> {
    let mut handles: Vec<Handle<Mesh>> = Vec::new();
    let mut query = world.query::<(Entity, &Mesh3d)>();
    let pairs: Vec<(Entity, Handle<Mesh>)> =
        query.iter(world).map(|(e, m)| (e, m.0.clone())).collect();
    for (entity, handle) in pairs {
        let mut current = entity;
        let derived = loop {
            if world.get::<MeshRefDerived>(current).is_some() {
                break true;
            }
            match world.get::<ChildOf>(current) {
                Some(parent) => current = parent.parent(),
                None => break false,
            }
        };
        if derived {
            handles.push(handle);
        }
    }
    let meshes = world.resource::<Assets<Mesh>>();
    handles
        .iter()
        .filter_map(|h| meshes.get(h))
        .filter_map(|mesh| match mesh.attribute(Mesh::ATTRIBUTE_POSITION) {
            Some(VertexAttributeValues::Float32x3(positions)) => positions
                .iter()
                .map(|p| p[1])
                .fold(None, |acc: Option<f32>, y| {
                    Some(acc.map_or(y, |a| a.max(y)))
                }),
            _ => None,
        })
        .collect()
}

fn instance_roots(world: &mut World) -> Vec<(Entity, Vec3)> {
    let mut query = world
        .query_filtered::<(Entity, &Transform), (With<PrefabInstance>, Without<PrefabStamped>)>();
    query.iter(world).map(|(e, t)| (e, t.translation)).collect()
}

pub(crate) fn probe_barrel(world: &mut World) {
    world.resource_mut::<BarrelProbe>().frame += 1;
    let frame = world.resource::<BarrelProbe>().frame;
    if frame == 1 {
        let _ = std::fs::create_dir_all(crate::probe_user::SHOT_DIR);
        // Clean slate (owner rule): probe-owned artifacts never leak between
        // runs — the keg prefab AND the barrel source with its identity.
        for stale in ["keg", "cask"] {
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
        // The processed-asset cache is a probe-owned artifact too: leaving it
        // behind makes the re-export a cache HIT on the second run, and the
        // "did the pipeline re-process?" check would pass or fail depending on
        // whether anyone ran the probe yesterday.
        let _ = std::fs::remove_dir_all(editor_scene::models::process_cache_dir(
            &world.resource::<ModelLibrary>().fs_root,
        ));
        let glb = glb_path(world);
        let _ = std::fs::create_dir_all(glb.parent().unwrap());
        let _ = std::fs::remove_file(editor_assets::identity::sidecar_path(&glb));
        // The artist drops the source asset in.
        std::fs::write(&glb, editor_assets::fixture::barrel_glb(1.0)).unwrap();
        info!("BARREL-PROBE armed — {}", glb.display());
    }
    match frame {
        // ── Boot: menu → game → editor, then import the drop ───────────────
        60 => tap_named(world, KeyCode::Enter, Key::Enter),
        120 => invoke(world, "core.toggle-editor"),
        160 => invoke(world, "asset.import"),
        200 => {
            let (entry, sidecar) = {
                let library = world.resource::<ModelLibrary>();
                let entry = library.entries.iter().find(|e| e.name == "barrel").cloned();
                let sidecar = editor_assets::identity::sidecar_path(&glb_path(world)).exists();
                (entry, sidecar)
            };
            check(world, entry.is_some(), "import indexed barrel.glb");
            check(world, sidecar, "import wrote the identity sidecar");
            world.resource_mut::<BarrelProbe>().uuid = entry.map(|e| e.uuid);
        }
        // ── Place the model from the insert palette ────────────────────────
        220 => tap(world, KeyCode::KeyI, "i"),
        260 => {
            for (code, ch) in [
                (KeyCode::KeyB, "b"),
                (KeyCode::KeyA, "a"),
                (KeyCode::KeyR, "r"),
                (KeyCode::KeyR, "r"),
            ] {
                tap(world, code, ch);
            }
        }
        290 => tap_named(world, KeyCode::Enter, Key::Enter),
        330 => {
            let placed = world.query::<(Entity, &MeshRef)>().iter(world).count();
            check(
                world,
                placed == 1,
                "Enter placed exactly one MeshRef entity",
            );
            let selected = world
                .query_filtered::<(), (With<MeshRef>, With<Selected>)>()
                .iter(world)
                .count();
            check(world, selected == 1, "the placed model is selected");
        }
        // Async gltf load: the barrel mesh appears under the reference.
        500 => {
            let tops = barrel_mesh_tops(world);
            let derived: Vec<(Entity, usize)> = world
                .query_filtered::<(Entity, Option<&Children>), With<MeshRefDerived>>()
                .iter(world)
                .map(|(e, c)| (e, c.map(|c| c.len()).unwrap_or(0)))
                .collect();
            let meshes = world.query::<&Mesh3d>().iter(world).count();
            info!("BARREL-PROBE diag: derived={derived:?} total_meshes={meshes} tops={tops:?}");
            check(
                world,
                tops.iter().any(|y| (y - 1.0).abs() < 0.01),
                "gltf scene spawned under the MeshRef (barrel height 1.0)",
            );
        }
        // ── Group into a prefab ("keg") — the asset becomes CONTENT ────────
        520 => tap(world, KeyCode::KeyG, "g"),
        560 => {
            for (code, ch) in [
                (KeyCode::KeyK, "k"),
                (KeyCode::KeyE, "e"),
                (KeyCode::KeyG, "g"),
            ] {
                tap(world, code, ch);
            }
        }
        // ── The stages the pipeline advertises actually run (spec §6) ──────
        // Both were dead in the real binary: no validators were ever
        // registered, so every import reported "0 problems" whatever it was
        // handed, and nothing ever called the Process stage at all.
        205 => {
            let validators = world.resource::<ValidatorCatalog>().validators.len();
            check(
                world,
                validators > 0,
                &format!("the Validate stage has validators registered ({validators})"),
            );
            let processors = world.resource::<ProcessorCatalog>().processors.len();
            check(
                world,
                processors > 0,
                &format!("the Process stage has processors registered ({processors})"),
            );
            let outputs = world.resource::<ProcessedAssets>().outputs.len();
            check(
                world,
                outputs > 0,
                &format!("and importing RAN it ({outputs} outputs)"),
            );
        }
        // The point of processing at import: the editor knows how big the
        // asset is before anything has loaded or spawned it.
        210 => {
            let bounds = world
                .resource::<ModelLibrary>()
                .entries
                .iter()
                .find(|entry| entry.name == "barrel")
                .and_then(|entry| entry.bounds);
            let measured = bounds.map(|b| b.size()).unwrap_or([0.0; 3]);
            check(
                world,
                bounds.is_some_and(|b| b.complete && b.triangles > 0),
                &format!("the library knows the barrel's size without spawning it ({measured:?})"),
            );
            world.resource_mut::<BarrelProbe>().bounds = bounds.map(|b| b.size());
        }
        600 => tap_named(world, KeyCode::Enter, Key::Enter),
        660 => {
            let roots = instance_roots(world).len();
            check(
                world,
                roots == 1,
                "g grouped the barrel into a keg instance",
            );
        }
        // ── Two more instances, separated by exact typed moves ─────────────
        // Esc first: `i` WITH a selection means add-component (owner grammar);
        // empty-handed `i` is the insert palette.
        670 => tap_named(world, KeyCode::Escape, Key::Escape),
        680 => tap(world, KeyCode::KeyI, "i"),
        710 => {
            for (code, ch) in [
                (KeyCode::KeyK, "k"),
                (KeyCode::KeyE, "e"),
                (KeyCode::KeyG, "g"),
            ] {
                tap(world, code, ch);
            }
        }
        740 => tap_named(world, KeyCode::Enter, Key::Enter),
        // One key per frame: the resolver reads ButtonInput, whose same-frame
        // iteration order is arbitrary — w/x/3 together resolve scrambled.
        790 => tap(world, KeyCode::KeyW, "w"),
        795 => tap(world, KeyCode::KeyX, "x"),
        800 => tap(world, KeyCode::Digit3, "3"),
        810 => tap_named(world, KeyCode::Enter, Key::Enter),
        830 => tap_named(world, KeyCode::Escape, Key::Escape),
        840 => tap(world, KeyCode::KeyI, "i"),
        870 => {
            for (code, ch) in [
                (KeyCode::KeyK, "k"),
                (KeyCode::KeyE, "e"),
                (KeyCode::KeyG, "g"),
            ] {
                tap(world, code, ch);
            }
        }
        900 => tap_named(world, KeyCode::Enter, Key::Enter),
        950 => tap(world, KeyCode::KeyW, "w"),
        955 => tap(world, KeyCode::KeyZ, "z"),
        960 => tap(world, KeyCode::Digit3, "3"),
        970 => tap_named(world, KeyCode::Enter, Key::Enter),
        // Settle + record the world as the user arranged it.
        1150 => {
            let roots = instance_roots(world);
            check(world, roots.len() == 3, "three keg instances placed");
            let spread = {
                let mut xs: Vec<f32> = roots.iter().map(|(_, p)| p.x).collect();
                xs.sort_by(f32::total_cmp);
                xs.last().copied().unwrap_or(0.0) - xs.first().copied().unwrap_or(0.0)
            };
            check(world, spread > 2.9, "typed moves separated the instances");
            let tops = barrel_mesh_tops(world);
            check(
                world,
                tops.iter().filter(|y| (**y - 1.0).abs() < 0.01).count() >= 3,
                "every instance renders the barrel mesh",
            );
            world.resource_mut::<BarrelProbe>().positions = roots;
            shot(world, "18-barrel-placed");
        }
        // ── The artist re-exports the source; the editor re-imports ────────
        1180 => {
            std::fs::write(glb_path(world), editor_assets::fixture::barrel_glb(2.5)).unwrap();
            invoke(world, "asset.import");
        }
        1220 => {
            let uuid_kept = {
                let probe_uuid = world.resource::<BarrelProbe>().uuid;
                let library = world.resource::<ModelLibrary>();
                probe_uuid.is_some()
                    && library
                        .entries
                        .iter()
                        .find(|e| e.name == "barrel")
                        .map(|e| e.uuid)
                        == probe_uuid
            };
            check(world, uuid_kept, "re-import preserved the asset UUID");
        }
        // THE inert check: the artist re-exported at a different scale, so the
        // cache must MISS and the recorded size must move. A second import of
        // unchanged bytes would only prove the cache works.
        1240 => {
            let (missed, size) = {
                let outputs = world.resource::<ProcessedAssets>();
                let library = world.resource::<ModelLibrary>();
                let entry = library.entries.iter().find(|e| e.name == "barrel");
                let missed = entry.is_some_and(|entry| {
                    outputs
                        .for_asset(entry.uuid)
                        .any(|output| !output.cache_hit)
                });
                (
                    missed,
                    entry.and_then(|entry| entry.bounds).map(|b| b.size()),
                )
            };
            check(
                world,
                missed,
                "the re-export re-processed rather than serving the cache",
            );
            let before = world.resource::<BarrelProbe>().bounds;
            let grew = match (before, size) {
                (Some(before), Some(after)) => (0..3).all(|axis| after[axis] > before[axis] * 2.0),
                _ => false,
            };
            check(
                world,
                grew,
                &format!("and the recorded size followed the re-export ({before:?} -> {size:?})"),
            );
        }
        // Reload + respawn settle, then THE verdict: content moved, layout didn't.
        1500 => {
            let tops = barrel_mesh_tops(world);
            check(
                world,
                !tops.is_empty() && tops.iter().all(|y| (y - 2.5).abs() < 0.01),
                &format!("re-import updated every instance mesh (tops {tops:?})"),
            );
            let now = instance_roots(world);
            let before = world.resource::<BarrelProbe>().positions.clone();
            let intact = before.len() == now.len()
                && before.iter().all(|(entity, position)| {
                    now.iter()
                        .any(|(e, p)| e == entity && (*p - *position).length() < 0.001)
                });
            check(world, intact, "instance transforms intact across re-import");
            shot(world, "19-barrel-reimported");
        }
        // ══ ACT 2: game-ready prefab — GLB + configuration (owner ask) ══════
        // Flatten the import to entities, add a collider + gameplay component
        // through the REAL add-component flow, prefab it, instance it.
        1560 => tap_named(world, KeyCode::Escape, Key::Escape),
        1580 => tap(world, KeyCode::KeyI, "i"),
        1610 => {
            for (code, ch) in [
                (KeyCode::KeyB, "b"),
                (KeyCode::KeyA, "a"),
                (KeyCode::KeyR, "r"),
                (KeyCode::KeyR, "r"),
            ] {
                tap(world, code, ch);
            }
        }
        1640 => tap_named(world, KeyCode::Enter, Key::Enter),
        1820 => invoke(world, "model.flatten"),
        1900 => {
            let nodes: Vec<String> = world
                .query::<(&editor_scene::models::MeshNode, &Name)>()
                .iter(world)
                .map(|(_, name)| name.as_str().to_string())
                .collect();
            check(
                world,
                nodes.len() == 2
                    && nodes.contains(&"barrel".into())
                    && nodes.contains(&"lid".into()),
                &format!("flatten materialized the gltf nodes as entities ({nodes:?})"),
            );
            let meshed = world
                .query_filtered::<(), (With<editor_scene::models::MeshNode>, With<Mesh3d>)>()
                .iter(world)
                .count();
            check(world, meshed == 2, "materialized nodes carry live meshes");
            let flat_selected = world
                .query_filtered::<(), (With<Selected>, Without<MeshRef>)>()
                .iter(world)
                .count();
            check(
                world,
                flat_selected == 1,
                "flatten removed MeshRef and kept the root selected",
            );
        }
        // Configuration via the real designer surface: i = add component.
        1920 => tap(world, KeyCode::KeyI, "i"),
        1950 => {
            for (code, ch) in [
                (KeyCode::KeyB, "b"),
                (KeyCode::KeyO, "o"),
                (KeyCode::KeyX, "x"),
                (KeyCode::KeyC, "c"),
            ] {
                tap(world, code, ch);
            }
        }
        1980 => tap_named(world, KeyCode::Enter, Key::Enter),
        2000 => {
            let flash = world
                .resource::<crate::statusbar::StatusFlash>()
                .text
                .clone();
            check(
                world,
                flash.contains("BoxCollider added"),
                "collider added via the add-component palette",
            );
            tap(world, KeyCode::KeyI, "i");
        }
        2030 => {
            for (code, ch) in [
                (KeyCode::KeyS, "s"),
                (KeyCode::KeyP, "p"),
                (KeyCode::KeyI, "i"),
                (KeyCode::KeyN, "n"),
            ] {
                tap(world, code, ch);
            }
        }
        2060 => tap_named(world, KeyCode::Enter, Key::Enter),
        2100 => {
            let flash = world
                .resource::<crate::statusbar::StatusFlash>()
                .text
                .clone();
            check(
                world,
                flash.contains("Spinner added"),
                "gameplay component added via the add-component palette",
            );
        }
        // Prefab it: the GLB + configuration becomes reusable game content.
        2120 => tap(world, KeyCode::KeyG, "g"),
        2150 => {
            for (code, ch) in [
                (KeyCode::KeyC, "c"),
                (KeyCode::KeyA, "a"),
                (KeyCode::KeyS, "s"),
                (KeyCode::KeyK, "k"),
            ] {
                tap(world, code, ch);
            }
        }
        2180 => tap_named(world, KeyCode::Enter, Key::Enter),
        2220 => tap_named(world, KeyCode::Escape, Key::Escape),
        2240 => tap(world, KeyCode::KeyI, "i"),
        2270 => {
            for (code, ch) in [
                (KeyCode::KeyC, "c"),
                (KeyCode::KeyA, "a"),
                (KeyCode::KeyS, "s"),
                (KeyCode::KeyK, "k"),
            ] {
                tap(world, code, ch);
            }
        }
        2300 => tap_named(world, KeyCode::Enter, Key::Enter),
        2460 => {
            let roots: Vec<(String, usize)> = {
                let ids: Vec<Entity> = world
                    .query_filtered::<Entity, (With<PrefabInstance>, Without<PrefabStamped>)>()
                    .iter(world)
                    .collect();
                ids.into_iter()
                    .map(|root| {
                        let name = world
                            .get::<Name>(root)
                            .map(|n| n.as_str().to_string())
                            .unwrap_or_default();
                        let kids = world.get::<Children>(root).map(|c| c.len()).unwrap_or(0);
                        (name, kids)
                    })
                    .collect()
            };
            let spinners = count_with_component(world, "Spinner");
            let colliders = count_with_component(world, "BoxCollider");
            let holders: Vec<(String, bool, Vec<String>)> =
                entities_with_component(world, "BoxCollider")
                    .into_iter()
                    .map(|entity| {
                        let mut ancestry = Vec::new();
                        let mut current = entity;
                        while let Some(parent) = world.get::<ChildOf>(current).map(|c| c.parent()) {
                            ancestry.push(
                                world
                                    .get::<Name>(parent)
                                    .map(|n| n.as_str().to_string())
                                    .unwrap_or_else(|| format!("{parent:?}")),
                            );
                            current = parent;
                        }
                        (
                            world
                                .get::<Name>(entity)
                                .map(|n| n.as_str().to_string())
                                .unwrap_or_default(),
                            world.get::<PrefabStamped>(entity).is_some(),
                            ancestry,
                        )
                    })
                    .collect();
            info!(
                "BARREL-PROBE diag2: roots={roots:?} spinners={spinners} colliders={colliders} holders={holders:?}"
            );
            // Two cask instances, each stamped GAME-READY: mesh nodes with live
            // meshes, collider + gameplay component on the root.
            let stamped_nodes = world
                .query_filtered::<(), (With<editor_scene::models::MeshNode>, With<PrefabStamped>)>()
                .iter(world)
                .count();
            check(
                world,
                stamped_nodes == 4,
                &format!("both cask instances stamped the mesh nodes ({stamped_nodes}/4)"),
            );
            let live = world
                .query_filtered::<(), (
                    With<editor_scene::models::MeshNode>,
                    With<PrefabStamped>,
                    With<Mesh3d>,
                )>()
                .iter(world)
                .count();
            check(world, live == 4, "every stamped node resolved its mesh");
            let configured = count_with_component(world, "BoxCollider");
            check(
                world,
                configured == 2,
                &format!("both cask roots carry the collider config ({configured}/2)"),
            );
            shot(world, "20-gameready-cask");
        }
        2520 => {
            let failures = world.resource::<BarrelProbe>().failures.clone();
            if failures.is_empty() {
                info!("BARREL-PROBE PASS: the barrel workflow end-to-end");
                world.write_message(AppExit::Success);
            } else {
                error!("BARREL-PROBE FAILED: {failures:?}");
                world.write_message(AppExit::error());
            }
        }
        _ => {}
    }
}

/// Count entities carrying a component the probe crate cannot name (game-crate
/// types like BoxCollider) — resolved through the reflection registry.
/// (`ComponentInfo::name()` is redacted without bevy's `debug` feature, so
/// name matching on component infos silently never matches.)
fn count_with_component(world: &mut World, short_name: &str) -> usize {
    entities_with_component(world, short_name).len()
}

fn entities_with_component(world: &mut World, short_name: &str) -> Vec<Entity> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let type_id = registry
        .read()
        .get_with_short_type_path(short_name)
        .map(|registration| registration.type_id());
    let Some(type_id) = type_id else {
        return Vec::new();
    };
    let Some(id) = world.components().get_valid_id(type_id) else {
        return Vec::new();
    };
    let mut query = bevy::ecs::query::QueryBuilder::<Entity>::new(world)
        .with_id(id)
        .build();
    query.iter(world).collect()
}
