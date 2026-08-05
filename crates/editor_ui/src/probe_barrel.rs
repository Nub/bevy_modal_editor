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
use editor_scene::models::{MeshRef, MeshRefDerived, ModelLibrary};
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
        let _ = std::fs::remove_file("prefabs/keg.prefab.ron");
        let _ = std::fs::remove_file("prefabs/keg.prefab.ron.bak");
        let mut library = world.resource_mut::<PrefabLibrary>();
        let ids: Vec<_> = library
            .prefabs
            .iter()
            .filter(|(_, def)| def.name.eq_ignore_ascii_case("keg"))
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            library.prefabs.remove(&id);
        }
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
        1560 => {
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
