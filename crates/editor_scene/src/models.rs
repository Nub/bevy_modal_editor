//! Model assets in the scene (M4-D12, spec §6 barrel workflow): imported
//! GLB/GLTF sources are referenced BY UUID via `MeshRef` — the scene file
//! carries the reference only, never mesh data, so references survive artist
//! re-exports (the identity sidecar keeps the UUID stable while the content
//! hash moves).
//!
//! The visual subtree is DERIVED: the resolver spawns the gltf scene under the
//! referencing entity with no `SceneId`s, so it is never serialized and is
//! rebuilt whenever the source's content hash changes (`asset.import` rescan).
//! Selection works because viewport picking already walks mesh hits up to the
//! nearest `SceneId` ancestor.

use bevy::gltf::GltfAssetLabel;
use bevy::prelude::*;
use bevy::world_serialization::{WorldAsset, WorldAssetRoot};
use editor_core::ValidatorCatalog;
use editor_core::prelude::*;
use std::path::PathBuf;
use uuid::Uuid;

/// Scene-side reference: which imported model this entity renders. Serialized
/// with the scene BY UUID — never by path, never by value.
#[derive(Component, Reflect, Clone, Copy, PartialEq, Debug, Default)]
#[reflect(Component, Default)]
pub struct MeshRef(pub Uuid);

/// A materialized gltf node (`model.flatten`): references its mesh + material
/// INSIDE the imported source by labeled asset path — fully serializable,
/// resolved to live handles by `resolve_mesh_nodes`. This is what makes an
/// import game-ready: each node is a real entity that colliders and gameplay
/// components attach to.
#[derive(Component, Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Component, Default)]
pub struct MeshNode {
    pub model: Uuid,
    /// Labeled mesh path ("models/barrel.glb#Mesh0/Primitive0").
    pub mesh: String,
    /// Labeled material path ("" = default material).
    pub material: String,
}

/// What an imported source IS — placement and pickers filter on this.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    /// GLB/GLTF — placeable in the scene via `MeshRef`.
    Model,
    /// Image — referenced by materials (D11 texture slots).
    Texture,
}

/// One imported source asset the editor knows about.
#[derive(Clone, Debug)]
pub struct ModelEntry {
    pub uuid: Uuid,
    pub kind: EntryKind,
    /// Display name (file stem).
    pub name: String,
    /// AssetServer-relative path ("models/barrel.glb").
    pub asset_path: String,
    /// blake3 of the source at last import — resolution keys off this.
    pub content_hash: String,
}

/// The imported-models index, rebuilt by `asset.import` (and once at startup).
#[derive(Resource)]
pub struct ModelLibrary {
    pub entries: Vec<ModelEntry>,
    /// Filesystem root of the asset tree (the directory AssetServer reads).
    pub fs_root: PathBuf,
    /// Bumped on every rescan that changed anything — resolvers key off this.
    pub generation: u64,
}

impl Default for ModelLibrary {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            fs_root: assets_fs_root(),
            generation: 0,
        }
    }
}

impl ModelLibrary {
    pub fn get(&self, uuid: &Uuid) -> Option<&ModelEntry> {
        self.entries.iter().find(|e| &e.uuid == uuid)
    }
}

/// Mirror Bevy's `FileAssetReader` root resolution so the scan sees exactly
/// the tree the AssetServer serves: `BEVY_ASSET_ROOT`, else the manifest dir
/// (cargo run), else the executable's directory — joined with "assets".
pub fn assets_fs_root() -> PathBuf {
    let base = std::env::var_os("BEVY_ASSET_ROOT")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("CARGO_MANIFEST_DIR").map(PathBuf::from))
        .or_else(|| {
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        })
        .unwrap_or_default();
    base.join("assets")
}

/// Import subtrees inside the asset root — the scan covers both.
pub const MODELS_DIR: &str = "models";
pub const TEXTURES_DIR: &str = "textures";

/// Scan + import every source under `<assets>/models` and `<assets>/textures`:
/// assigns/refreshes identity sidecars, runs the validator catalog, returns
/// entries + problems. Pure with respect to the world — callers surface the
/// problems.
pub fn scan_models(
    fs_root: &std::path::Path,
    validators: &[editor_api::validate::ValidatorDef],
) -> (Vec<ModelEntry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut problems = Vec::new();
    let kind_of = |p: &std::path::Path| match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("glb") | Some("gltf") => Some(EntryKind::Model),
        Some("png") | Some("jpg") | Some("jpeg") | Some("ktx2") => Some(EntryKind::Texture),
        _ => None,
    };
    for dir_name in [MODELS_DIR, TEXTURES_DIR] {
        let Ok(read) = std::fs::read_dir(fs_root.join(dir_name)) else {
            continue; // subtree absent — nothing imported from it
        };
        let mut sources: Vec<PathBuf> = read
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| kind_of(p).is_some())
            .collect();
        sources.sort(); // deterministic order
        for source in sources {
            let kind = kind_of(&source).expect("filtered above");
            let identity = match editor_assets::import_file(&source) {
                Ok(identity) => identity,
                Err(e) => {
                    problems.push(format!("{}: {e}", source.display()));
                    continue;
                }
            };
            match std::fs::read(&source) {
                Ok(bytes) => {
                    for problem in editor_assets::run_validators(&source, &bytes, validators) {
                        problems.push(format!("{:?}: {}", problem.severity, problem.message));
                    }
                }
                Err(e) => problems.push(format!("{}: {e}", source.display())),
            }
            let name = source
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("asset")
                .to_string();
            let asset_path = format!(
                "{dir_name}/{}",
                source.file_name().and_then(|s| s.to_str()).unwrap_or("")
            );
            entries.push(ModelEntry {
                uuid: identity.uuid,
                kind,
                name,
                asset_path,
                content_hash: identity.content_hash,
            });
        }
    }
    (entries, problems)
}

pub(crate) struct ModelsFeature;

impl EditorFeature for ModelsFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("models", "Imported Models")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.component::<MeshRef>()
            .component::<MeshNode>()
            .action(
                ActionDef::new("asset.import", "Import Assets (rescan models)")
                    .describe("Scan assets/models, assign identities, validate, refresh instances")
                    .context("normal"),
            )
            .action(
                ActionDef::new("model.flatten", "Flatten Model To Entities")
                    .describe(
                        "Materialize the selected model's gltf nodes as real, editable \
                         entities (colliders and components attach per node)",
                    )
                    .context("normal"),
            );
    }
}

#[derive(Resource, Default)]
pub(crate) struct ImportRequested(pub bool);

#[derive(Resource, Default)]
pub(crate) struct FlattenRequested(pub bool);

/// Live root handles per imported source (Gltf for models, Image for
/// textures). Without these the root asset is dropped (only labeled/derived
/// sub-assets are referenced) and `AssetServer::reload` has nothing to reload
/// — re-imports would silently serve stale content forever.
#[derive(Resource, Default)]
pub(crate) struct ModelHandles(pub std::collections::HashMap<Uuid, UntypedHandle>);

pub(crate) fn collect_model_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut requested: ResMut<ImportRequested>,
    mut flatten: ResMut<FlattenRequested>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "asset.import" => requested.0 = true,
            "model.flatten" => flatten.0 = true,
            _ => {}
        }
    }
}

/// Startup + on-demand rescan. Reloads changed sources through the AssetServer
/// so already-spawned gltf scenes refresh (Bevy respawns scene instances on
/// asset modification); the resolver additionally respawns subtrees whose
/// content hash moved.
pub(crate) fn perform_import(
    mut requested: ResMut<ImportRequested>,
    mut library: ResMut<ModelLibrary>,
    validators: Option<Res<ValidatorCatalog>>,
    assets: Option<Res<AssetServer>>,
    mut handles: ResMut<ModelHandles>,
    mut feedback: MessageWriter<super::SceneIoFeedback>,
) {
    // Headless test worlds have no AssetServer — imports stand down there.
    let (Some(validators), Some(assets)) = (validators, assets) else {
        return;
    };
    if !requested.0 {
        return;
    }
    requested.0 = false;
    let (entries, problems) = scan_models(&library.fs_root, &validators.validators);
    for problem in &problems {
        warn!("asset import: {problem}");
    }
    // Reload sources whose bytes changed — cached handles would otherwise
    // serve the stale content forever. Keyed by PATH, not uuid: a re-minted
    // identity (deleted sidecar) still points at the same cached asset path,
    // and the cache goes stale all the same.
    let reload_paths: Vec<String> = entries
        .iter()
        .filter(|entry| {
            library.entries.iter().any(|old| {
                old.asset_path == entry.asset_path && old.content_hash != entry.content_hash
            })
        })
        .map(|entry| entry.asset_path.clone())
        .collect();
    let changed = library.entries.len() != entries.len()
        || library.entries.iter().zip(&entries).any(|(a, b)| {
            a.uuid != b.uuid || a.content_hash != b.content_hash || a.asset_path != b.asset_path
        });
    library.entries = entries;
    if changed {
        library.generation += 1;
    }
    // Root handles keep sources reloadable; prune ones that left the tree.
    let live: std::collections::HashSet<Uuid> = library.entries.iter().map(|e| e.uuid).collect();
    handles.0.retain(|uuid, _| live.contains(uuid));
    for entry in &library.entries {
        // Textures are deliberately NOT preloaded. The asset server keys
        // handles by PATH, so the FIRST loader of a file decides its loader
        // settings and every later `load_with_settings` for that path gets the
        // existing handle with its settings DISCARDED. A material has to load
        // each map in the colour space its slot declares — linear for normal,
        // metallic-roughness and occlusion — with repeat wrapping so tiling
        // repeats instead of smearing the edge texels. Parking a
        // default-settings handle here would win that race and silently
        // gamma-decode every data map, which is exactly the bug the slot table
        // exists to prevent. Models still preload: a Gltf has no such setting.
        if entry.kind == EntryKind::Model {
            handles.0.entry(entry.uuid).or_insert_with(|| {
                assets
                    .load::<bevy::gltf::Gltf>(entry.asset_path.clone())
                    .untyped()
            });
        }
    }
    for path in reload_paths {
        info!("asset import: reloading changed source {path}");
        assets.reload(path);
    }
    feedback.write(super::SceneIoFeedback {
        message: format!(
            "imported {} model{} \u{b7} {} problem{}",
            library.entries.len(),
            if library.entries.len() == 1 { "" } else { "s" },
            problems.len(),
            if problems.len() == 1 { "" } else { "s" },
        ),
        success: problems.is_empty(),
    });
}

pub(crate) fn import_at_startup(mut requested: ResMut<ImportRequested>) {
    requested.0 = true;
}

/// Book-keeping on the referencing entity: which uuid+hash the spawned subtree
/// was built from, and the derived child carrying it.
#[derive(Component)]
pub(crate) struct MeshRefResolved {
    uuid: Uuid,
    content_hash: String,
    child: Entity,
}

/// Marker on the derived (never-serialized) gltf subtree root.
#[derive(Component)]
pub struct MeshRefDerived;

/// Spawn/refresh the derived gltf subtree under every `MeshRef` entity.
/// Re-resolves when the component changes OR the library moves (re-import).
pub(crate) fn resolve_mesh_refs(
    mut commands: Commands,
    library: Res<ModelLibrary>,
    assets: Option<Res<AssetServer>>,
    refs: Query<(Entity, &MeshRef, Option<&MeshRefResolved>)>,
    mut removed: RemovedComponents<MeshRef>,
    resolved_q: Query<&MeshRefResolved>,
    scene_events: Option<MessageReader<AssetEvent<WorldAsset>>>,
    derived: Query<(Entity, &WorldAssetRoot, &ChildOf), With<MeshRefDerived>>,
) {
    let Some(assets) = assets else { return };
    // Undo/removal: the derived subtree goes with the reference.
    for entity in removed.read() {
        if let Ok(resolved) = resolved_q.get(entity) {
            commands.entity(resolved.child).despawn();
            commands.entity(entity).remove::<MeshRefResolved>();
        }
    }
    // Re-import completion: `reload` finishes ASYNC, after the hash-keyed
    // respawn already grabbed the cached scene — when the asset actually
    // changes, force every derived subtree built from it through the spawn
    // path again so instances render the re-exported content.
    let modified: Vec<AssetId<WorldAsset>> = scene_events
        .map(|mut events| {
            events
                .read()
                .filter_map(|event| match event {
                    AssetEvent::Modified { id } => Some(*id),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    if !modified.is_empty() {
        for (child, root, child_of) in &derived {
            if modified.contains(&root.0.id()) {
                debug!("mesh_ref: scene modified — respawning subtree under {child:?}");
                commands.entity(child).despawn();
                commands
                    .entity(child_of.parent())
                    .remove::<MeshRefResolved>();
            }
        }
    }
    for (entity, mesh_ref, resolved) in &refs {
        let Some(entry) = library.get(&mesh_ref.0) else {
            // Unknown uuid (library not scanned yet, or dangling ref): leave
            // any existing visual alone; a later import resolves it.
            continue;
        };
        let current =
            resolved.is_some_and(|r| r.uuid == mesh_ref.0 && r.content_hash == entry.content_hash);
        if current {
            continue;
        }
        if let Some(resolved) = resolved {
            commands.entity(resolved.child).despawn();
        }
        debug!(
            "mesh_ref: spawning {} under {entity:?} (hash {})",
            entry.asset_path,
            &entry.content_hash[..8]
        );
        let child = commands
            .spawn((
                MeshRefDerived,
                WorldAssetRoot(
                    assets.load(GltfAssetLabel::Scene(0).from_asset(entry.asset_path.clone())),
                ),
                Transform::IDENTITY,
                ChildOf(entity),
            ))
            .id();
        commands.entity(entity).insert(MeshRefResolved {
            uuid: mesh_ref.0,
            content_hash: entry.content_hash.clone(),
            child,
        });
    }
}

/// `model.flatten` (spec §6 "flatten-on-import"): materialize the SELECTED
/// model's spawned gltf nodes as real scene entities — one per mesh-bearing
/// node, root-relative transform, `MeshNode` carrying the labeled asset paths.
/// One undoable transaction: spawn nodes, reparent under the root, remove the
/// root's `MeshRef` (whose derived subtree despawns with it).
pub(crate) fn perform_flatten(world: &mut World) {
    if !std::mem::take(&mut world.resource_mut::<FlattenRequested>().0) {
        return;
    }
    let selected: Vec<(Entity, SceneId, Uuid)> = {
        let mut query = world.query_filtered::<(Entity, &SceneId, &MeshRef), With<Selected>>();
        query
            .iter(world)
            .map(|(entity, id, mesh_ref)| (entity, *id, mesh_ref.0))
            .collect()
    };
    if selected.is_empty() {
        world.write_message(super::SceneIoFeedback {
            message: "select a placed model to flatten".into(),
            success: false,
        });
        return;
    }
    let mut flattened = 0usize;
    for (root_entity, root_id, model) in selected {
        let Some(derived_root) = world
            .get::<MeshRefResolved>(root_entity)
            .map(|resolved| resolved.child)
        else {
            continue; // still loading — nothing spawned to materialize yet
        };
        // Every mesh-bearing descendant, with its pose ACCUMULATED up to the
        // referencing root — intermediate gltf nodes collapse away (flatten).
        let mut nodes: Vec<(String, Transform, String, String)> = Vec::new();
        let mut stack: Vec<(Entity, Transform)> = vec![(derived_root, Transform::IDENTITY)];
        while let Some((entity, acc)) = stack.pop() {
            let local = world.get::<Transform>(entity).copied().unwrap_or_default();
            let acc = if entity == derived_root {
                Transform::IDENTITY
            } else {
                acc.mul_transform(local)
            };
            if let Some(mesh) = world.get::<Mesh3d>(entity) {
                let mesh_path = mesh.0.path().map(|p| p.to_string()).unwrap_or_default();
                let material_path = world
                    .get::<MeshMaterial3d<StandardMaterial>>(entity)
                    .and_then(|m| m.0.path())
                    .map(|p| p.to_string())
                    .unwrap_or_default();
                // bevy_gltf names primitive entities "mesh.material" — the
                // authored NODE name lives on the parent node entity.
                let name = world
                    .get::<ChildOf>(entity)
                    .map(|c| c.parent())
                    .filter(|parent| *parent != derived_root)
                    .and_then(|parent| world.get::<Name>(parent))
                    .or_else(|| world.get::<Name>(entity))
                    .map(|n| n.as_str().to_string())
                    .unwrap_or_else(|| "node".into());
                if mesh_path.is_empty() {
                    warn!("model.flatten: node {name:?} has an unlabeled mesh — skipped");
                } else {
                    nodes.push((name, acc, mesh_path, material_path));
                }
            }
            if let Some(children) = world.get::<Children>(entity) {
                for child in children.iter() {
                    stack.push((child, acc));
                }
            }
        }
        if nodes.is_empty() {
            continue;
        }
        let mut ops = Vec::new();
        for (name, transform, mesh_path, material_path) in nodes {
            let id = SceneId::random();
            ops.push(Op::Spawn {
                id,
                components: vec![
                    Box::new(transform).into_partial_reflect(),
                    Box::new(Name::new(name)).into_partial_reflect(),
                    Box::new(MeshNode {
                        model,
                        mesh: mesh_path,
                        material: material_path,
                    })
                    .into_partial_reflect(),
                ],
            });
            ops.push(Op::Reparent {
                target: id,
                parent: Some(root_id),
            });
            flattened += 1;
        }
        ops.push(Op::Remove {
            target: root_id,
            type_path: <MeshRef as bevy::reflect::TypePath>::type_path().into(),
        });
        world.resource_mut::<EditQueue>().0.push(Transaction {
            label: "Flatten Model".into(),
            gesture: None,
            ops,
        });
    }
    let success = flattened > 0;
    world.write_message(super::SceneIoFeedback {
        message: if success {
            format!(
                "flattened {flattened} node{} to entities",
                if flattened == 1 { "" } else { "s" }
            )
        } else {
            "model still loading — try again in a moment".into()
        },
        success,
    });
}

/// Attach live mesh/material handles to `MeshNode` entities (insert, scene
/// load, undo/redo all funnel through Changed); removal strips them.
pub(crate) fn resolve_mesh_nodes(
    mut commands: Commands,
    assets: Option<Res<AssetServer>>,
    nodes: Query<(Entity, &MeshNode), Changed<MeshNode>>,
    mut removed: RemovedComponents<MeshNode>,
) {
    let Some(assets) = assets else { return };
    for entity in removed.read() {
        if let Ok(mut e) = commands.get_entity(entity) {
            e.remove::<(Mesh3d, MeshMaterial3d<StandardMaterial>)>();
        }
    }
    for (entity, node) in &nodes {
        if node.mesh.is_empty() {
            continue;
        }
        commands
            .entity(entity)
            .insert(Mesh3d(assets.load(node.mesh.clone())));
        if !node.material.is_empty() {
            commands
                .entity(entity)
                .insert(MeshMaterial3d::<StandardMaterial>(
                    assets.load(node.material.clone()),
                ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // D12 (import half): the scan assigns stable identity, survives re-export
    // with the SAME uuid + a NEW hash, and surfaces validator problems.
    #[test]
    fn scan_imports_and_keeps_identity() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("assets");
        std::fs::create_dir_all(root.join(MODELS_DIR)).unwrap();
        let glb = root.join(MODELS_DIR).join("barrel.glb");
        std::fs::write(&glb, editor_assets::fixture::barrel_glb(1.0)).unwrap();

        let validators = editor_assets::builtin_validators();
        let (entries, problems) = scan_models(&root, &validators);
        assert!(problems.is_empty(), "{problems:?}");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "barrel");
        assert_eq!(entries[0].asset_path, "models/barrel.glb");
        let first = entries[0].clone();

        // Artist re-export: identity survives, hash moves.
        std::fs::write(&glb, editor_assets::fixture::barrel_glb(2.0)).unwrap();
        let (entries, _) = scan_models(&root, &validators);
        assert_eq!(entries[0].uuid, first.uuid, "uuid survives re-export");
        assert_ne!(entries[0].content_hash, first.content_hash);

        // A broken source is a PROBLEM, never a silent skip.
        std::fs::write(root.join(MODELS_DIR).join("broken.glb"), b"not a glb").unwrap();
        let (entries, problems) = scan_models(&root, &validators);
        assert_eq!(entries.len(), 2, "broken source still gets identity");
        assert!(
            problems.iter().any(|p| p.contains("broken.glb")),
            "{problems:?}"
        );
    }
}
