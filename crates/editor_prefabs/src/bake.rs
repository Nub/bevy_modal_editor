//! Bake runner (spec §6, M4-D8): derives registered bakers' artifacts from
//! prefab templates into `bake/` — CACHES, never source of truth.
//!
//! - Artifact key = blake3(template RON) × baker version: any template edit or
//!   baker bump changes the filename, so a stale artifact can never be served
//!   by accident — it simply isn't there under the expected name.
//! - `bake_all` deletes superseded artifacts for the same prefab × baker as it
//!   writes fresh ones; deleting the whole `bake/` dir reproduces bit-for-bit
//!   (pinned by test).
//! - Staleness is SURFACED: a library change flags missing artifacts in the
//!   statusbar; `prefab.bake` (palette: "Bake Now") clears them.

use crate::{PrefabDef, PrefabLibrary};
use bevy::prelude::*;
use editor_core::prelude::*;
use std::path::PathBuf;

pub const BAKE_DIR: &str = "bake";

/// Where artifacts land — tests point this at a tempdir.
#[derive(Resource)]
pub struct BakeDir(pub PathBuf);

impl Default for BakeDir {
    fn default() -> Self {
        Self(PathBuf::from(BAKE_DIR))
    }
}

#[derive(Resource, Default)]
pub struct BakeRequests {
    pub bake: bool,
}

/// Result of a bake pass — feedback + CLI exit status.
#[derive(Default, Debug, PartialEq, Eq)]
pub struct BakeReport {
    pub written: usize,
    pub fresh: usize,
    pub skipped: usize,
    pub errors: Vec<String>,
}

fn template_ron(world: &World, def: &PrefabDef) -> Option<String> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    def.template.to_ron(&registry).ok()
}

fn artifact_path(
    base: &std::path::Path,
    baker: &editor_api::bake::BakerDef,
    def: &PrefabDef,
    hash: &str,
) -> PathBuf {
    base.join(baker.id.as_str())
        .join(format!("{}-v{}-{}.bake", def.id, baker.version, hash))
}

fn content_hash(template: &str) -> String {
    blake3::hash(template.as_bytes()).to_hex()[..16].to_string()
}

/// Bake every registered baker over every library prefab. Deterministic and
/// idempotent: fresh artifacts are left untouched, superseded ones removed.
pub fn bake_all(world: &mut World) -> BakeReport {
    let base = world.resource::<BakeDir>().0.clone();
    let bakers = world.resource::<BakerCatalog>().bakers.clone();
    let prefabs: Vec<(uuid::Uuid, String)> = world
        .resource::<PrefabLibrary>()
        .prefabs
        .keys()
        .map(|id| (*id, String::new()))
        .collect();
    let mut report = BakeReport::default();

    for (prefab_id, _) in prefabs {
        let Some((name, template)) = ({
            let library = world.resource::<PrefabLibrary>();
            library
                .prefabs
                .get(&prefab_id)
                .map(|def| (def.name.clone(), template_ron(world, def)))
        }) else {
            continue;
        };
        let Some(template) = template else {
            report
                .errors
                .push(format!("{name}: template serialization failed"));
            continue;
        };
        let hash = content_hash(&template);
        for baker in &bakers {
            let library = world.resource::<PrefabLibrary>();
            let Some(def) = library.prefabs.get(&prefab_id) else {
                continue;
            };
            let path = artifact_path(&base, baker, def, &hash);
            if path.exists() {
                report.fresh += 1;
                continue;
            }
            let cx = editor_api::bake::BakeCx {
                prefab_id,
                prefab_name: &name,
                template_ron: &template,
            };
            match (baker.bake)(&cx) {
                Ok(Some(bytes)) => {
                    let _ = std::fs::create_dir_all(path.parent().unwrap());
                    // Superseded artifacts for this prefab × baker die here —
                    // exactly one artifact per pair may exist.
                    let prefix = format!("{prefab_id}-");
                    if let Ok(entries) = std::fs::read_dir(path.parent().unwrap()) {
                        for entry in entries.flatten() {
                            let stale = entry.file_name().to_string_lossy().starts_with(&prefix);
                            if stale {
                                let _ = std::fs::remove_file(entry.path());
                            }
                        }
                    }
                    if let Err(e) = std::fs::write(&path, bytes) {
                        report.errors.push(format!("{name}/{}: {e}", baker.id));
                    } else {
                        report.written += 1;
                    }
                }
                Ok(None) => report.skipped += 1,
                Err(e) => report.errors.push(format!("{name}/{}: {e}", baker.id)),
            }
        }
    }
    report
}

/// Prefab × baker pairs whose CURRENT-key artifact is missing (stale or never
/// baked). Cheap: filename existence only, no baking.
pub fn stale_bakes(world: &mut World) -> Vec<String> {
    let base = world.resource::<BakeDir>().0.clone();
    let bakers = world.resource::<BakerCatalog>().bakers.clone();
    let entries: Vec<(uuid::Uuid, String)> = {
        let library = world.resource::<PrefabLibrary>();
        library
            .prefabs
            .values()
            .map(|def| (def.id, def.name.clone()))
            .collect()
    };
    let mut stale = Vec::new();
    for (prefab_id, name) in entries {
        let Some(template) = ({
            let library = world.resource::<PrefabLibrary>();
            library
                .prefabs
                .get(&prefab_id)
                .and_then(|d| template_ron(world, d))
        }) else {
            continue;
        };
        let hash = content_hash(&template);
        for baker in &bakers {
            let library = world.resource::<PrefabLibrary>();
            let Some(def) = library.prefabs.get(&prefab_id) else {
                continue;
            };
            if !artifact_path(&base, baker, def, &hash).exists() {
                stale.push(format!("{name}/{}", baker.id));
            }
        }
    }
    stale
}

/// Library changes surface staleness in the statusbar (never silently served).
pub(crate) fn watch_bake_staleness(world: &mut World) {
    let generation = world.resource::<PrefabLibrary>().generation;
    let last = world.resource::<LastBakeCheck>().0;
    if generation == last {
        return;
    }
    world.resource_mut::<LastBakeCheck>().0 = generation;
    if world.resource::<BakerCatalog>().bakers.is_empty() {
        return;
    }
    let stale = stale_bakes(world);
    if !stale.is_empty() {
        world.write_message(editor_scene::SceneIoFeedback {
            message: format!("{} bake(s) stale — run Bake Now", stale.len()),
            success: false,
        });
    }
    // Kit coherence rides the same cadence (D10): loud, specific, non-fatal.
    let warnings = crate::sockets::kit_coherence(world.resource::<crate::PrefabLibrary>());
    if let Some(first) = warnings.first() {
        world.write_message(editor_scene::SceneIoFeedback {
            message: first.clone(),
            success: false,
        });
    }
}

#[derive(Resource, Default)]
pub(crate) struct LastBakeCheck(pub u64);

pub(crate) fn perform_bake(world: &mut World) {
    if !std::mem::take(&mut world.resource_mut::<BakeRequests>().bake) {
        return;
    }
    let report = bake_all(world);
    let message = if report.errors.is_empty() {
        format!(
            "baked {} artifact(s) · {} fresh · {} n/a",
            report.written, report.fresh, report.skipped
        )
    } else {
        format!("bake finished with errors: {}", report.errors.join("; "))
    };
    world.write_message(editor_scene::SceneIoFeedback {
        message,
        success: report.errors.is_empty(),
    });
}

/// EDITOR_BAKE=1: headless batch mode (`editor bake` CLI) — bake everything on
/// the first frame after startup, print the report, exit with status.
pub(crate) fn headless_bake_mode(world: &mut World, mut done: Local<bool>) {
    if *done || std::env::var("EDITOR_BAKE").is_err() {
        return;
    }
    *done = true;
    let report = bake_all(world);
    println!(
        "bake: {} written · {} fresh · {} n/a · {} error(s)",
        report.written,
        report.fresh,
        report.skipped,
        report.errors.len()
    );
    for error in &report.errors {
        eprintln!("bake error: {error}");
    }
    world.write_message(if report.errors.is_empty() {
        bevy::app::AppExit::Success
    } else {
        bevy::app::AppExit::error()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{barrel_prefab, test_app};

    fn baked_app(dir: &std::path::Path) -> App {
        let mut app = test_app();
        app.insert_resource(BakeDir(dir.to_path_buf()));
        // Startup loaded the PROJECT's prefabs/ directory: this test counts
        // artifacts, so it must bake its own fixture and nothing else. Any
        // prefab an owner authors would otherwise change the count.
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .clear();
        let prefab = barrel_prefab();
        let id = prefab.id;
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(id, prefab);
        app
    }

    // D8 contract: delete all bake output → bake reproduces bit-for-bit;
    // fresh artifacts untouched; template edits re-key and supersede.
    #[test]
    fn bake_reproduces_bit_for_bit() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = baked_app(dir.path());

        let first = bake_all(app.world_mut());
        assert_eq!(
            first.written, 1,
            "digest baker wrote one artifact: {first:?}"
        );
        let artifact = |base: &std::path::Path| {
            let baker_dir = base.join("test.digest");
            let entry = std::fs::read_dir(baker_dir)
                .unwrap()
                .next()
                .unwrap()
                .unwrap();
            (entry.path(), std::fs::read(entry.path()).unwrap())
        };
        let (_, bytes_before) = artifact(dir.path());

        // Idempotent: nothing rewritten while fresh.
        let second = bake_all(app.world_mut());
        assert_eq!((second.written, second.fresh), (0, 1));

        // Nuke everything → reproduced byte-identically.
        std::fs::remove_dir_all(dir.path().join("test.digest")).unwrap();
        assert_eq!(
            stale_bakes(app.world_mut()).len(),
            1,
            "missing artifact = stale"
        );
        let third = bake_all(app.world_mut());
        assert_eq!(third.written, 1);
        let (_, bytes_after) = artifact(dir.path());
        assert_eq!(bytes_before, bytes_after, "bit-for-bit reproduction");

        // Template edit re-keys: old artifact superseded, exactly one remains.
        {
            let mut library = app.world_mut().resource_mut::<PrefabLibrary>();
            let def = library.prefabs.values_mut().next().unwrap();
            def.name = "Renamed".into();
            let records = def
                .template
                .records()
                .map(|(id, parent, c)| (id, parent, c.iter().map(|v| v.to_dynamic()).collect()))
                .collect::<Vec<_>>();
            // touch the template by roundtripping with an extra record
            let mut records = records;
            records.push((
                SceneId::random(),
                None,
                vec![Box::new(Transform::from_xyz(9.0, 0.0, 0.0)).into_partial_reflect()],
            ));
            def.template = editor_scene::snapshot_from_parts(records);
        }
        assert!(
            !stale_bakes(app.world_mut()).is_empty(),
            "template edit = stale"
        );
        let fourth = bake_all(app.world_mut());
        assert_eq!(fourth.written, 1);
        let count = std::fs::read_dir(dir.path().join("test.digest"))
            .unwrap()
            .count();
        assert_eq!(
            count, 1,
            "superseded artifact removed — one per prefab \u{d7} baker"
        );
    }
}
