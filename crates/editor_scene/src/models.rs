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
use editor_api::validate::{AssetProblem, ProblemSource, Severity, Stage};
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
    /// How big it is, from the Process stage, WITHOUT loading or spawning it.
    ///
    /// The live `Aabb` stays authoritative once a model is loaded; this is the
    /// answer to the same question BEFORE that, which is the window in which
    /// socket generation, ghost sizing and footprint proposals currently get
    /// nothing at all. Two sources that disagree would be a v1 pattern (§11);
    /// one source that arrives earlier is not.
    pub bounds: Option<editor_assets::ModelBounds>,
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

/// Where processed outputs are cached: a SIBLING of the asset root, never
/// inside it. A cache under `assets/` would be served by the asset server and
/// re-scanned as source on the next import — the pipeline would start eating
/// its own output.
pub fn process_cache_dir(fs_root: &std::path::Path) -> PathBuf {
    fs_root
        .parent()
        .unwrap_or(fs_root)
        .join(".editor-cache")
        .join("process")
}

/// One processed output: which processor produced what, for which asset.
///
/// The Cook stage (spec §6) is a manifest over exactly this — uuid to path —
/// which is why the record is kept rather than thrown away after the toast.
#[derive(Clone, Debug, PartialEq)]
pub struct ProcessedOutput {
    pub uuid: Uuid,
    pub processor: String,
    pub output: PathBuf,
    /// False when this run actually did the work.
    pub cache_hit: bool,
}

/// What the pipeline produced for the whole tree.
#[derive(Default)]
pub struct IngestReport {
    pub entries: Vec<ModelEntry>,
    pub problems: Vec<AssetProblem>,
    /// How many files the walk actually looked at. "0 problems" with no
    /// denominator is the sentence this module was written to kill — it reads
    /// identically whether the tree is clean or the scan never found it.
    pub scanned: usize,
    pub outputs: Vec<ProcessedOutput>,
}

/// The stages, as data: what to check with, what to run, where to cache.
pub struct IngestConfig<'a> {
    pub validators: &'a [editor_api::validate::ValidatorDef],
    pub processors: &'a [editor_api::pipeline::ProcessorDef],
    pub cache_dir: &'a std::path::Path,
}

/// Import → Validate → Process over every source under `<assets>/models` and
/// `<assets>/textures`, recursively (spec §6).
///
/// Import assigns or refreshes the identity sidecar; Validate runs the
/// validator catalog; Process runs every registered processor whose declared
/// extensions match, through the content-hash + version keyed cache. All three
/// see the same bytes, read once.
///
/// Nothing here touches the world — the caller surfaces the problems — and
/// nothing here aborts on a bad asset: a source that fails a stage still gets
/// an identity and still appears, because an asset that vanishes from the
/// library because a processor disliked it is the worst possible failure mode.
///
/// **Nothing is dropped in silence.** A file the pipeline cannot place and no
/// processor claims becomes a PROBLEM saying so. Asset packs ship `.fbx`,
/// `.tga` and `.tif`, and "imported 0 models · 0 problems" for a folder full
/// of art is the least debuggable sentence this editor could print.
pub fn ingest(fs_root: &std::path::Path, config: &IngestConfig) -> IngestReport {
    let mut report = IngestReport::default();
    for dir_name in [MODELS_DIR, TEXTURES_DIR] {
        let mut sources = Vec::new();
        // Recursive: a purchased pack arrives as `models/dungeon/walls/*.glb`,
        // which is the shape every marketplace ships. A flat scan finds the
        // directory, cannot make an asset of it, and says nothing at all.
        collect_sources(
            fs_root,
            &fs_root.join(dir_name),
            &mut sources,
            &mut report.problems,
        );
        sources.sort(); // deterministic order
        report.scanned += sources.len();
        for source in sources {
            let extension = extension_of(&source);
            let kind = kind_of(&source);
            let claimed = config
                .processors
                .iter()
                .any(|def| processor_claims(def, &extension));
            let relative = display_relative(fs_root, &source);
            if kind.is_none() && !claimed {
                // INFO, not warning. A pack shipping `.fbx` and `.blend`
                // alongside its `.glb` is a fact about the pack, not a defect
                // in it — and a warning you are meant to ignore teaches you to
                // ignore warnings.
                report.problems.push(AssetProblem {
                    stage: Stage::Import,
                    source: ProblemSource::Ingest,
                    severity: Severity::Info,
                    path: relative.clone(),
                    uuid: None,
                    message: format!("nothing imports or processes .{extension}"),
                });
                continue;
            }
            let mut bounds = None;
            let identity = match editor_assets::import_file(&source) {
                Ok(identity) => identity,
                Err(e) => {
                    // ERROR: the `continue` below drops this file from the
                    // library entirely, so every reference to it dangles.
                    // There is no uuid to name yet — that is what failed.
                    report.problems.push(AssetProblem {
                        stage: Stage::Import,
                        source: ProblemSource::Ingest,
                        severity: Severity::Error,
                        path: relative.clone(),
                        uuid: None,
                        message: format!("{e}"),
                    });
                    continue;
                }
            };
            match std::fs::read(&source) {
                Ok(bytes) => {
                    for problem in editor_assets::run_validators(&source, &bytes, config.validators)
                    {
                        // The severity comes through UNCHANGED. It used to be
                        // stringified into the message, which is why a stray
                        // `.fbx` and a corrupt mesh looked identical.
                        report.problems.push(AssetProblem::from_validator(
                            problem,
                            relative.clone(),
                            Some(identity.uuid),
                        ));
                    }
                    process_one(
                        &source,
                        &bytes,
                        &extension,
                        identity.uuid,
                        &relative,
                        config,
                        &mut report,
                    );
                    bounds = read_bounds(&report, identity.uuid);
                }
                Err(e) => report.problems.push(AssetProblem {
                    stage: Stage::Import,
                    source: ProblemSource::Ingest,
                    severity: Severity::Error,
                    path: relative.clone(),
                    uuid: Some(identity.uuid),
                    message: format!("{e}"),
                }),
            }
            // A source only a processor claims (a `.tif` waiting on a
            // converter) is a real asset of the pipeline and NOT a member of
            // the library: the editor cannot load it, and offering it in the
            // palette would place something that never appears.
            let Some(kind) = kind else { continue };
            let name = source
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("asset")
                .to_string();
            report.entries.push(ModelEntry {
                uuid: identity.uuid,
                kind,
                name,
                asset_path: relative,
                content_hash: identity.content_hash,
                bounds,
            });
        }
    }
    // A broken `.glb` produces the SAME parse error twice, because both gltf
    // validators parse the file and both report the failure. The string list
    // hid it; typed records with `PartialEq` do not, and the pair is always
    // adjacent, so consecutive dedup is enough.
    report.problems.dedup();
    report
}

/// Pull the built-in bounds record out of what the Process stage just
/// produced, so the library carries the number in memory.
///
/// The library knowing the id of ONE built-in processor is deliberate: bounds
/// are wanted on a gesture path, where reading a file is not acceptable.
/// Everything else a processor produces stays a path in `ProcessedAssets`,
/// which is what the Cook stage will read.
fn read_bounds(report: &IngestReport, uuid: Uuid) -> Option<editor_assets::ModelBounds> {
    let output = report
        .outputs
        .iter()
        .rev()
        .find(|o| o.uuid == uuid && o.processor == editor_assets::BOUNDS_PROCESSOR)?;
    let bytes = std::fs::read(&output.output).ok()?;
    ron::de::from_bytes(&bytes).ok()
}
/// Every file under `dir`, depth-first. Unreadable subtrees are problems, not
/// silence — a permissions error that hides half a pack has to be loud.
fn collect_sources(
    fs_root: &std::path::Path,
    dir: &std::path::Path,
    out: &mut Vec<PathBuf>,
    problems: &mut Vec<AssetProblem>,
) {
    let read = match std::fs::read_dir(dir) {
        Ok(read) => read,
        // Absent is normal: a project may have no textures at all.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            problems.push(AssetProblem {
                stage: Stage::Import,
                source: ProblemSource::Ingest,
                severity: Severity::Error,
                path: display_relative(fs_root, dir),
                uuid: None,
                message: format!("{e}"),
            });
            return;
        }
    };
    let mut children: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    children.sort();
    for child in children {
        if child.is_dir() {
            collect_sources(fs_root, &child, out, problems);
        } else if !is_sidecar(&child) {
            out.push(child);
        }
    }
}

/// Import sidecars are the pipeline's own book-keeping, not source assets.
fn is_sidecar(path: &std::path::Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.ends_with(".import.ron") || name.ends_with(".meta"))
}

fn extension_of(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default()
}

/// What the EDITOR can load. A file outside this list may still be pipeline
/// input; it is simply not a library entry.
fn kind_of(path: &std::path::Path) -> Option<EntryKind> {
    match extension_of(path).as_str() {
        "glb" | "gltf" => Some(EntryKind::Model),
        "png" | "jpg" | "jpeg" | "ktx2" => Some(EntryKind::Texture),
        _ => None,
    }
}

/// Asset-server-relative path, forward-slashed on every platform because that
/// is what the asset server takes.
fn display_relative(fs_root: &std::path::Path, source: &std::path::Path) -> String {
    source
        .strip_prefix(fs_root)
        .unwrap_or(source)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// An EMPTY extension list means "every extension", matching the validator
/// registry exactly (`run_validators`). The two registries sitting next to each
/// other in one pipeline must not read the same declaration in opposite ways —
/// a game registering an any-asset processor by the validator's precedent would
/// otherwise get a processor that silently never runs, which is the very bug
/// this wiring exists to end.
fn processor_claims(def: &editor_api::pipeline::ProcessorDef, extension: &str) -> bool {
    def.extensions.is_empty() || def.extensions.contains(&extension)
}

/// The Process stage for one source: every processor that claims this
/// extension, in registration order.
fn process_one(
    source: &std::path::Path,
    bytes: &[u8],
    extension: &str,
    uuid: Uuid,
    relative: &str,
    config: &IngestConfig,
    report: &mut IngestReport,
) {
    for def in config.processors {
        if !processor_claims(def, extension) {
            continue;
        }
        match editor_assets::process_asset(source, bytes, def, config.cache_dir) {
            Ok(outcome) => report.outputs.push(ProcessedOutput {
                uuid,
                processor: def.id.to_string(),
                output: outcome.output,
                cache_hit: outcome.cache_hit,
            }),
            // A processor that fails is a PROBLEM, not an import failure: the
            // asset is still in the project, and the designer needs to know
            // which stage refused it rather than watching it disappear.
            // A refusing processor keeps the asset (the comment above says
            // why) so it is a WARNING. An Io failure is different in kind: the
            // cache itself is broken, and every asset is about to hit it.
            Err(e) => {
                let severity = match e {
                    editor_assets::ProcessError::Io(_) => Severity::Error,
                    editor_assets::ProcessError::Processor { .. } => Severity::Warning,
                };
                report.problems.push(AssetProblem {
                    stage: Stage::Process,
                    source: ProblemSource::Processor(def.id.to_string()),
                    severity,
                    path: relative.to_string(),
                    uuid: Some(uuid),
                    message: format!("{e}"),
                });
            }
        }
    }
}

pub(crate) struct ModelsFeature;

impl EditorFeature for ModelsFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("models", "Imported Models")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        // THE BUILT-IN VALIDATORS, REGISTERED. They existed, they were tested,
        // and nothing ever put them in the catalog — so every import in the
        // real binary ran `run_validators` against an EMPTY list and reported
        // "0 problems" whatever it was handed. A stage that cannot fail is not
        // a stage. (spec §6 Validate, M4-D2.)
        for validator in editor_assets::builtin_validators() {
            reg.validator(validator);
        }
        // And the Process stage's first processor. Bounds are readable from
        // the JSON chunk alone — the same bytes the cache is keyed on — so
        // this is the one kind of processing that is honest under a cache key
        // that hashes a single file (spec §6 Process, M4-D3).
        reg.processor(editor_assets::bounds::processor());
        // A placed import: same source asset. A materialized node below it is
        // the same NODE of the same model, which is a narrower family.
        reg.identity::<MeshRef>(editor_api::identity::priority::MODEL, "", "same model");
        reg.identity::<MeshNode>(
            editor_api::identity::priority::MESH_NODE,
            "mesh",
            "same mesh",
        );
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

/// What the Process stage produced this session, by asset.
///
/// Kept rather than discarded because it is exactly the input the Cook stage
/// needs (spec §6: "a manifest maps UUID → cooked path"), and because a
/// pipeline whose results are invisible is indistinguishable from one that
/// never ran — which is how the stage came to be dead code in the first place.
#[derive(Resource, Default)]
pub struct ProcessedAssets {
    pub outputs: Vec<ProcessedOutput>,
}

impl ProcessedAssets {
    /// Everything produced for one asset.
    pub fn for_asset(&self, uuid: Uuid) -> impl Iterator<Item = &ProcessedOutput> {
        self.outputs.iter().filter(move |o| o.uuid == uuid)
    }
}

/// What the last import found, kept rather than logged and dropped.
///
/// It is NOT a field on `ModelLibrary`, for two reasons. `library.generation`
/// bumps only when the ENTRIES changed and it drives mesh-ref resolution — a
/// rescan that fixes a warning must not respawn every model in the level. And
/// half these problems are about files that are not library members at all: an
/// unclaimed `.fbx` and a source whose identity could not be written both have
/// no entry to hang off.
///
/// Empty means "no problems OR the import never ran" — `perform_import` stands
/// down entirely in a headless world with no AssetServer. `scanned` is the
/// denominator that tells those two apart.
#[derive(Resource, Default)]
pub struct AssetProblems {
    pub problems: Vec<AssetProblem>,
    /// Bumped only when the set actually changed (compared by value, never by
    /// serializing) — a `ResMut` deref alone marks a resource changed, which is
    /// exactly why `LevelValidation` carries its own counter too.
    pub generation: u64,
    pub scanned: usize,
}

impl AssetProblems {
    pub fn count(&self, severity: Severity) -> usize {
        self.problems
            .iter()
            .filter(|p| p.severity == severity)
            .count()
    }

    /// Everything said about one asset.
    pub fn for_asset(&self, uuid: Uuid) -> impl Iterator<Item = &AssetProblem> {
        self.problems.iter().filter(move |p| p.uuid == Some(uuid))
    }

    /// The worst thing said about one asset, which is what a row shows.
    pub fn worst_for(&self, uuid: Uuid) -> Option<Severity> {
        self.for_asset(uuid)
            .map(|p| p.severity)
            .max_by_key(|s| match s {
                Severity::Info => 0,
                Severity::Warning => 1,
                Severity::Error => 2,
            })
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
    processors: Option<Res<ProcessorCatalog>>,
    assets: Option<Res<AssetServer>>,
    mut handles: ResMut<ModelHandles>,
    mut processed: ResMut<ProcessedAssets>,
    mut asset_problems: ResMut<AssetProblems>,
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
    let cache_dir = process_cache_dir(&library.fs_root);
    let no_processors = Vec::new();
    let report = ingest(
        &library.fs_root,
        &IngestConfig {
            validators: &validators.validators,
            processors: processors
                .as_ref()
                .map(|catalog| catalog.processors.as_slice())
                .unwrap_or(&no_processors),
            cache_dir: &cache_dir,
        },
    );
    let (entries, problems, report_outputs, scanned) = (
        report.entries,
        report.problems,
        report.outputs,
        report.scanned,
    );
    // Routed by severity, mirroring how level validation logs. A flat `warn!`
    // for everything is why an ignored `.fbx` used to read exactly like a
    // corrupt mesh.
    for problem in &problems {
        let line = format!(
            "asset {}: {}: {}",
            problem.stage.label(),
            problem.path,
            problem.message
        );
        match problem.severity {
            Severity::Error => error!("{line}"),
            Severity::Warning => warn!("{line}"),
            Severity::Info => info!("{line}"),
        }
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
    let ran = report_outputs.iter().filter(|o| !o.cache_hit).count();
    let cached = report_outputs.len() - ran;
    processed.outputs = report_outputs;
    // Compared by VALUE, so an unchanged rescan does not bump the generation
    // and the panels do not rebuild. `is_changed()` would be useless here: the
    // `ResMut` deref above already marks it changed every import.
    let errors = problems
        .iter()
        .filter(|p| p.severity == Severity::Error)
        .count();
    let warnings = problems
        .iter()
        .filter(|p| p.severity == Severity::Warning)
        .count();
    if asset_problems.problems != problems {
        asset_problems.problems = problems;
        asset_problems.generation += 1;
    }
    asset_problems.scanned = scanned;
    feedback.write(super::SceneIoFeedback {
        message: format!(
            "imported {} of {scanned} file{} \u{b7} processed {ran} ({cached} cached) \u{b7} {errors} error{}, {warnings} warning{}",
            library.entries.len(),
            if scanned == 1 { "" } else { "s" },
            if errors == 1 { "" } else { "s" },
            if warnings == 1 { "" } else { "s" },
        ),
        // An ignored `.fbx` used to paint a clean import red. Only an ERROR
        // means something did not arrive.
        success: errors == 0,
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
                // Also the kernel's DERIVED marker, so the edit engine stops
                // its capture walk at this boundary instead of relying on
                // "no SceneId below here" staying true.
                editor_api::edits::Derived,
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
    use editor_api::pipeline::ProcessorDef;
    use editor_api::prelude::ProcessorId;

    fn validate_only<'a>(
        validators: &'a [editor_api::validate::ValidatorDef],
        cache: &'a std::path::Path,
    ) -> IngestConfig<'a> {
        IngestConfig {
            validators,
            processors: &[],
            cache_dir: cache,
        }
    }

    fn counting_processor(extensions: &'static [&'static str]) -> ProcessorDef {
        ProcessorDef {
            id: ProcessorId::new_static("test.size"),
            name: "Byte count",
            version: 1,
            extensions,
            process: |cx| Ok(format!("{}", cx.bytes.len()).into_bytes()),
        }
    }

    fn failing_processor() -> ProcessorDef {
        ProcessorDef {
            id: ProcessorId::new_static("test.refuses"),
            name: "Refuses everything",
            version: 1,
            extensions: &["glb"],
            process: |_| Err("nope".into()),
        }
    }

    fn corpus() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("assets");
        std::fs::create_dir_all(root.join(MODELS_DIR)).unwrap();
        std::fs::write(
            root.join(MODELS_DIR).join("barrel.glb"),
            editor_assets::fixture::barrel_glb(1.0),
        )
        .unwrap();
        (dir, root)
    }

    // D12 (import half): the scan assigns stable identity, survives re-export
    // with the SAME uuid + a NEW hash, and surfaces validator problems.
    #[test]
    fn scan_imports_and_keeps_identity() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        let glb = root.join(MODELS_DIR).join("barrel.glb");

        let validators = editor_assets::builtin_validators();
        let report = ingest(&root, &validate_only(&validators, &cache));
        assert!(report.problems.is_empty(), "{:?}", report.problems);
        assert_eq!(report.entries.len(), 1);
        assert_eq!(report.entries[0].name, "barrel");
        assert_eq!(report.entries[0].asset_path, "models/barrel.glb");
        let first = report.entries[0].clone();

        // Artist re-export: identity survives, hash moves.
        std::fs::write(&glb, editor_assets::fixture::barrel_glb(2.0)).unwrap();
        let report = ingest(&root, &validate_only(&validators, &cache));
        assert_eq!(
            report.entries[0].uuid, first.uuid,
            "uuid survives re-export"
        );
        assert_ne!(report.entries[0].content_hash, first.content_hash);

        // A broken source is a PROBLEM, never a silent skip.
        std::fs::write(root.join(MODELS_DIR).join("broken.glb"), b"not a glb").unwrap();
        let report = ingest(&root, &validate_only(&validators, &cache));
        assert_eq!(report.entries.len(), 2, "broken source still gets identity");
        assert!(
            report.problems.iter().any(|p| {
                p.path.contains("broken.glb")
                    && p.severity == Severity::Error
                    && p.stage == Stage::Validate
            }),
            "{:?}",
            report.problems
        );
    }

    // D3, the wiring half: importing RUNS the registered processors, and says
    // what it produced. The stage was a tested library that nothing called —
    // this is the test that would have caught that.
    #[test]
    fn importing_runs_the_registered_processors() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        let validators = editor_assets::builtin_validators();
        let processors = vec![counting_processor(&["glb"])];
        let config = IngestConfig {
            validators: &validators,
            processors: &processors,
            cache_dir: &cache,
        };

        let report = ingest(&root, &config);
        assert_eq!(report.outputs.len(), 1, "the processor ran");
        assert_eq!(report.outputs[0].processor, "test.size");
        assert_eq!(report.outputs[0].uuid, report.entries[0].uuid);
        assert!(!report.outputs[0].cache_hit, "first run does the work");
        assert!(report.outputs[0].output.exists(), "and left its output");

        // Second import, nothing changed: the cache answers.
        let again = ingest(&root, &config);
        assert!(again.outputs[0].cache_hit, "unchanged input is a cache hit");
        assert_eq!(again.outputs[0].output, report.outputs[0].output);
    }

    /// A processor only claims what it declares. Otherwise a texture
    /// compressor would be handed a .glb and fail on every import.
    #[test]
    fn a_processor_only_sees_the_extensions_it_claims() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        let validators = editor_assets::builtin_validators();
        let processors = vec![counting_processor(&["png"])];
        let report = ingest(
            &root,
            &IngestConfig {
                validators: &validators,
                processors: &processors,
                cache_dir: &cache,
            },
        );
        assert_eq!(report.entries.len(), 1, "the model still imported");
        assert!(report.outputs.is_empty(), "but no png processor ran on it");
    }

    /// An asset that a processor refuses is still an asset. Anything else and
    /// one strict processor can delete a designer's model from the project.
    #[test]
    fn a_failing_processor_is_a_problem_not_a_lost_asset() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        let validators = editor_assets::builtin_validators();
        let processors = vec![failing_processor(), counting_processor(&["glb"])];
        let report = ingest(
            &root,
            &IngestConfig {
                validators: &validators,
                processors: &processors,
                cache_dir: &cache,
            },
        );
        assert_eq!(report.entries.len(), 1, "the asset survived");
        assert!(
            report.problems.iter().any(|p| {
                matches!(&p.source, ProblemSource::Processor(id) if id == "test.refuses")
                    && p.severity == Severity::Warning
                    && p.stage == Stage::Process
            }),
            "and said which processor refused it: {:?}",
            report.problems
        );
        assert_eq!(report.outputs.len(), 1, "the processors after it still ran");
    }

    /// The cache must never live inside the tree the scan walks, or the
    /// pipeline starts importing its own output.
    #[test]
    fn the_process_cache_sits_outside_the_asset_root() {
        let root = PathBuf::from("/project/assets");
        let cache = process_cache_dir(&root);
        assert!(!cache.starts_with(&root), "{cache:?}");
    }

    /// A purchased pack arrives foldered. A flat scan finds a directory it
    /// cannot make an asset of and says nothing — "imported 0 models · 0
    /// problems" for a folder full of art.
    #[test]
    fn the_scan_walks_subfolders() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        let nested = root.join(MODELS_DIR).join("dungeon").join("walls");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(
            nested.join("wall.glb"),
            editor_assets::fixture::barrel_glb(1.0),
        )
        .unwrap();

        let validators = editor_assets::builtin_validators();
        let report = ingest(&root, &validate_only(&validators, &cache));
        let paths: Vec<&str> = report
            .entries
            .iter()
            .map(|entry| entry.asset_path.as_str())
            .collect();
        assert!(
            paths.contains(&"models/dungeon/walls/wall.glb"),
            "nested source imported at its asset-server path: {paths:?}"
        );
        assert!(paths.contains(&"models/barrel.glb"), "{paths:?}");
    }

    /// Half the packs on the market ship `.fbx`, and this project's own asset
    /// folder has a `.tif` in it. Dropping them without a word is the least
    /// debuggable thing ingest can do.
    #[test]
    fn a_file_nothing_claims_is_a_problem_not_a_silence() {
        let (dir, root) = corpus();
        std::fs::write(root.join(MODELS_DIR).join("chair.fbx"), b"fbx bytes").unwrap();
        let cache = dir.path().join("cache");

        let validators = editor_assets::builtin_validators();
        let report = ingest(&root, &validate_only(&validators, &cache));
        assert!(
            report.problems.iter().any(|p| {
                p.path.contains("chair.fbx")
                    && p.severity == Severity::Info
                    && p.source == ProblemSource::Ingest
            }),
            "{:?}",
            report.problems
        );
    }

    /// The converter case: a processor claiming `.tif` makes the file pipeline
    /// input even though the editor cannot load one. It must be processed and
    /// must NOT appear in the library — offering it in the palette would place
    /// something that never renders.
    #[test]
    fn a_processor_can_claim_a_format_the_editor_cannot_load() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        std::fs::write(root.join(MODELS_DIR).join("metal.tif"), b"tif bytes").unwrap();
        let validators = editor_assets::builtin_validators();

        let processors = vec![counting_processor(&["tif"])];
        let report = ingest(
            &root,
            &IngestConfig {
                validators: &validators,
                processors: &processors,
                cache_dir: &cache,
            },
        );
        assert_eq!(report.outputs.len(), 1, "the converter saw it");
        assert!(
            !report.problems.iter().any(|p| p.path.contains("metal.tif")),
            "and it is no longer an ignored file: {:?}",
            report.problems
        );
        assert!(
            report.entries.iter().all(|e| e.name != "metal"),
            "but it is not offered as placeable content"
        );
    }

    /// The two registries sit in one pipeline; an empty extension list must
    /// not mean "everything" in Validate and "nothing" in Process.
    #[test]
    fn an_empty_extension_list_means_every_extension_in_both_registries() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        let validators = editor_assets::builtin_validators();
        let processors = vec![counting_processor(&[])];
        let report = ingest(
            &root,
            &IngestConfig {
                validators: &validators,
                processors: &processors,
                cache_dir: &cache,
            },
        );
        assert_eq!(report.outputs.len(), 1, "an any-asset processor ran");
    }
    /// Its own book-keeping is not source content.
    #[test]
    fn sidecars_are_not_imported_as_assets() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        let validators = editor_assets::builtin_validators();
        let report = ingest(&root, &validate_only(&validators, &cache));
        assert!(root.join(MODELS_DIR).join("barrel.glb.import.ron").exists());
        let again = ingest(&root, &validate_only(&validators, &cache));
        assert_eq!(again.entries.len(), report.entries.len(), "no new entries");
        assert!(
            !again.problems.iter().any(|p| p.path.contains("import.ron")),
            "and the sidecar is not an ignored file: {:?}",
            again.problems
        );
    }

    /// The bug class that hid for a whole milestone: a stage whose registry is
    /// EMPTY runs happily and reports nothing. `builtin_validators()` existed,
    /// was tested, and was registered by nobody — so every import in the real
    /// binary validated against an empty list and said "0 problems" whatever it
    /// was handed. Assert the catalogs have contents, not just that the code
    /// that would fill them compiles.
    #[test]
    fn the_pipeline_stages_are_actually_registered() {
        let mut registry = editor_api::feature::FeatureRegistry::default();
        registry.register_feature(&ModelsFeature);
        assert!(
            !registry.validators.is_empty(),
            "Validate has validators registered"
        );
        assert!(
            !registry.processors.is_empty(),
            "Process has processors registered"
        );
        assert!(
            registry
                .processors
                .iter()
                .any(|(_, def)| def.id.as_str() == editor_assets::BOUNDS_PROCESSOR),
            "including the bounds processor the library reads"
        );
    }

    /// The library carries the size of an asset it has never spawned — which
    /// is the whole point of measuring at import.
    #[test]
    fn the_library_knows_how_big_a_model_is_without_loading_it() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        let validators = editor_assets::builtin_validators();
        let processors = vec![editor_assets::bounds::processor()];
        let config = IngestConfig {
            validators: &validators,
            processors: &processors,
            cache_dir: &cache,
        };
        let report = ingest(&root, &config);
        let bounds = report.entries[0].bounds.expect("bounds recorded");
        assert!(bounds.complete && bounds.triangles > 0);
        let small = bounds.size();

        // The artist re-exports at 2.5: the cache MISSES and the number moves.
        // A second import of unchanged bytes would prove only that the cache
        // works; this proves the pipeline is measuring the file in front of it.
        std::fs::write(
            root.join(MODELS_DIR).join("barrel.glb"),
            editor_assets::fixture::barrel_glb(2.5),
        )
        .unwrap();
        let report = ingest(&root, &config);
        assert!(!report.outputs[0].cache_hit, "changed content re-processes");
        let large = report.entries[0].bounds.expect("bounds recorded").size();
        for axis in 0..3 {
            assert!(large[axis] > small[axis] * 2.0, "{small:?} -> {large:?}");
        }
    }

    // Typed problems (spec §6 "Asset browser"): the pipeline's findings are
    // state the editor can show, not a line in a log nobody reads.

    #[test]
    fn severity_survives_the_walk() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        std::fs::write(root.join(MODELS_DIR).join("chair.fbx"), b"fbx bytes").unwrap();
        std::fs::write(root.join(MODELS_DIR).join("empty.glb"), b"").unwrap();
        let validators = editor_assets::builtin_validators();
        let report = ingest(&root, &validate_only(&validators, &cache));

        // An unclaimed extension is INFO. A pack that ships .fbx beside its
        // .glb is a fact about the pack, and a warning nobody can act on
        // teaches people to ignore warnings.
        let fbx = report
            .problems
            .iter()
            .find(|p| p.path.contains("chair.fbx"))
            .expect("the ignored file said nothing");
        assert_eq!(fbx.severity, Severity::Info);
        assert_eq!(fbx.stage, Stage::Import);

        // An empty source is an ERROR, and it names the validator that said so.
        let empty = report
            .problems
            .iter()
            .find(|p| {
                p.path.contains("empty.glb")
                    && matches!(&p.source, ProblemSource::Validator(id) if id.as_str() == "asset.nonempty")
            })
            .expect("an empty glb passed validation");
        assert_eq!(empty.severity, Severity::Error);
        assert_eq!(empty.stage, Stage::Validate);
        // Before this change both of these stringified into one flat list and
        // an ignored file was indistinguishable from a broken one.
        assert_ne!(fbx.severity, empty.severity);
    }

    #[test]
    fn a_problem_names_the_asset_it_is_about() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        std::fs::write(root.join(MODELS_DIR).join("empty.glb"), b"").unwrap();
        let validators = editor_assets::builtin_validators();
        let report = ingest(&root, &validate_only(&validators, &cache));
        let entry = report
            .entries
            .iter()
            .find(|e| e.name == "empty")
            .expect("a source that fails validation still gets an entry");
        // The uuid is the join. A problem that only carried a path could not
        // be shown against the row the browser already draws.
        assert!(
            report
                .problems
                .iter()
                .any(|p| p.uuid == Some(entry.uuid) && p.severity == Severity::Error),
            "{:?}",
            report.problems
        );
        // And the path spelling matches the entry's, so the join works either
        // way round — no second path convention.
        assert!(
            report.problems.iter().any(|p| p.path == entry.asset_path),
            "problem path {:?} does not match entry path {:?}",
            report.problems.iter().map(|p| &p.path).collect::<Vec<_>>(),
            entry.asset_path
        );
    }

    /// Both gltf validators parse the file, so both report the same parse
    /// failure. As strings that was invisible; as records it would be two
    /// identical rows in the browser for one broken file.
    #[test]
    fn one_broken_file_is_one_problem_not_two() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        std::fs::write(root.join(MODELS_DIR).join("broken.glb"), b"not a glb").unwrap();
        let validators = editor_assets::builtin_validators();
        let report = ingest(&root, &validate_only(&validators, &cache));
        let about_broken: Vec<_> = report
            .problems
            .iter()
            .filter(|p| p.path.contains("broken.glb"))
            .collect();
        assert_eq!(
            about_broken.len(),
            1,
            "the same parse failure was reported twice: {about_broken:?}"
        );
    }

    /// "0 problems" reads identically whether the tree is clean or the scan
    /// never found it. The denominator is what tells those apart.
    #[test]
    fn the_walk_reports_what_it_looked_at() {
        let (dir, root) = corpus();
        let cache = dir.path().join("cache");
        std::fs::write(root.join(MODELS_DIR).join("chair.fbx"), b"fbx bytes").unwrap();
        let validators = editor_assets::builtin_validators();
        let report = ingest(&root, &validate_only(&validators, &cache));
        // barrel.glb + chair.fbx — the ignored file is still a file the walk
        // looked at, which is the whole point of counting.
        assert_eq!(report.scanned, 2);
        assert_eq!(report.entries.len(), 1, "and only one became an asset");
    }
}
