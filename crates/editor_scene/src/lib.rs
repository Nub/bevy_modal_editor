//! `editor_scene` — versioned serialization, stable IDs, atomic scene I/O (spec §5).
//!
//! - **Envelope from day one**: `{ format_version, entities }`; loading an unknown
//!   version is a typed error, never a guess (migration chains hang off this).
//! - **BSN-semantic payload**: per-entity records of reflected components + parent
//!   references by UUID — no expanded trees, no `Entity` values (ledger #1/#3).
//! - **Deterministic output** (spike-4 serializer rules): entities sorted by UUID,
//!   components sorted by type path, stable pretty-RON — save→load→save is
//!   byte-identical (B4) and git-merge-friendly.
//! - **Atomic + non-destructive I/O** (B5): temp+rename with a `.bak`; loads parse and
//!   resolve fully into a staging snapshot before the world is touched.
//! - `SceneSnapshot` is THE scene capture type: save/load use it, and play/reset (M2
//!   task 11) snapshots through the exact same path — one choke point (spec §5).

use bevy::ecs::relationship::RelationshipHookMode;
use bevy::prelude::*;
use bevy::reflect::{PartialReflect, TypeRegistry};
use editor_core::prelude::*;
use serde::de::DeserializeSeed;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const FORMAT_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Data model
// ---------------------------------------------------------------------------

/// A fully-resolved scene capture: the single choke point for save, load, and
/// play/reset snapshots. Resolution happens at construction — applying cannot fail
/// halfway through.
pub struct SceneSnapshot {
    records: Vec<ResolvedRecord>,
}

pub(crate) struct ResolvedRecord {
    pub(crate) id: SceneId,
    pub(crate) parent: Option<SceneId>,
    pub(crate) components: Vec<Box<dyn PartialReflect>>,
}

#[derive(Debug)]
pub enum SceneError {
    Io(std::io::Error),
    /// Parse/resolve failure with full context in the message (entity, type path).
    Parse(String),
    UnsupportedVersion {
        found: u32,
        supported: u32,
    },
}

impl std::fmt::Display for SceneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "scene io: {e}"),
            Self::Parse(e) => write!(f, "scene parse: {e}"),
            Self::UnsupportedVersion { found, supported } => write!(
                f,
                "scene format version {found} is newer than supported {supported} — \
                 upgrade the editor"
            ),
        }
    }
}
impl std::error::Error for SceneError {}

// ---------------------------------------------------------------------------
// Capture / apply (world <-> snapshot)
// ---------------------------------------------------------------------------

fn clone_component(
    world: &World,
    registry: &TypeRegistry,
    entity: Entity,
    type_id: std::any::TypeId,
) -> Option<Box<dyn PartialReflect>> {
    let reflect_component = registry
        .get(type_id)?
        .data::<bevy::ecs::reflect::ReflectComponent>()?;
    let entity_ref = world.get_entity(entity).ok()?;
    Some(reflect_component.reflect(entity_ref)?.to_dynamic())
}

/// While set, scene save/open stand down (prefab edit mode owns the world).
#[derive(Resource, Default)]
pub struct SceneIoLock(pub bool);

/// Entities STAMPED from a prefab template (M4-D4): they exist for editing
/// (selection, gizmos) but are DERIVED state — scene capture excludes them, so
/// an instance can never serialize as its expanded tree (the v1 prefab sin).
#[derive(Component, Default, Clone)]
pub struct PrefabStamped;

/// Build a snapshot from parts (prefab authoring, create-from-selection).
pub fn snapshot_from_parts(
    records: Vec<(SceneId, Option<SceneId>, Vec<Box<dyn PartialReflect>>)>,
) -> SceneSnapshot {
    let mut records: Vec<ResolvedRecord> = records
        .into_iter()
        .map(|(id, parent, components)| ResolvedRecord {
            id,
            parent,
            components,
        })
        .collect();
    records.sort_by_key(|r| r.id.0);
    SceneSnapshot { records }
}

/// Iterate a snapshot's records (id, parent, components) — prefab stamping reads
/// templates through this without owning the serialization format.
impl SceneSnapshot {
    pub fn records(
        &self,
    ) -> impl Iterator<Item = (SceneId, Option<SceneId>, &[Box<dyn PartialReflect>])> {
        self.records
            .iter()
            .map(|r| (r.id, r.parent, r.components.as_slice()))
    }
}

/// Capture every `SceneId` entity's registered components (the one capture path).
pub fn capture_scene(world: &World) -> SceneSnapshot {
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    let editor_components = world.resource::<EditorComponents>();
    let index = world.resource::<SceneIndex>();

    let mut records: Vec<ResolvedRecord> = index
        .iter()
        .filter(|&(_, &entity)| world.get::<PrefabStamped>(entity).is_none())
        .map(|(id, &entity)| {
            let components = editor_components
                .types
                .iter()
                .filter_map(|reg| clone_component(world, &registry, entity, reg.type_id))
                .collect();
            let parent = world
                .get::<ChildOf>(entity)
                .and_then(|c| world.get::<SceneId>(c.parent()))
                .copied();
            ResolvedRecord {
                id: *id,
                parent,
                components,
            }
        })
        .collect();
    records.sort_by_key(|r| r.id.0);
    SceneSnapshot { records }
}

/// Replace the current scene with the snapshot. Only called with a fully-resolved
/// snapshot, so it cannot fail partway (B5's non-destructive guarantee lives in
/// `SceneSnapshot::from_ron`, which resolves BEFORE anyone calls this).
pub fn apply_scene(world: &mut World, snapshot: &SceneSnapshot, clear_history: bool) {
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();

    // Despawn the current scene.
    let current: Vec<Entity> = world
        .resource::<SceneIndex>()
        .iter()
        .map(|(_, &e)| e)
        .collect();
    for entity in current {
        if let Ok(entity_mut) = world.get_entity_mut(entity) {
            entity_mut.despawn();
        }
    }

    // Spawn all records, then wire parents.
    let mut spawned: HashMap<SceneId, Entity> = HashMap::new();
    for record in &snapshot.records {
        let entity = world.spawn(record.id).id();
        for value in &record.components {
            let Some(info) = value.get_represented_type_info() else {
                continue;
            };
            let Some(registration) = registry.get(info.type_id()) else {
                continue;
            };
            let Some(reflect_component) =
                registration.data::<bevy::ecs::reflect::ReflectComponent>()
            else {
                continue;
            };
            // A type present in a FILE is part of this project's save set —
            // re-adopt it so the next capture doesn't silently drop it
            // (runtime-adopted types survive restarts through their files).
            world
                .resource_mut::<editor_core::edits::EditorComponents>()
                .adopt(info.type_id(), registration.type_info().type_path());
            let Ok(mut entity_mut) = world.get_entity_mut(entity) else {
                continue;
            };
            reflect_component.apply_or_insert_mapped(
                &mut entity_mut,
                value.as_ref(),
                &registry,
                &mut (),
                RelationshipHookMode::Run,
            );
        }
        spawned.insert(record.id, entity);
    }
    for record in &snapshot.records {
        if let (Some(&child), Some(parent_id)) = (spawned.get(&record.id), record.parent)
            && let Some(&parent) = spawned.get(&parent_id)
        {
            world.entity_mut(child).insert(ChildOf(parent));
        }
    }

    // A loaded scene starts with clean history; play-reset PRESERVES it (B10 —
    // SceneId-targeted ops survive respawn).
    if clear_history {
        world.resource_mut::<History>().clear();
    }
}

// ---------------------------------------------------------------------------
// Serialize / deserialize (snapshot <-> text)
// ---------------------------------------------------------------------------

impl SceneSnapshot {
    pub fn to_ron(&self, registry: &TypeRegistry) -> Result<String, SceneError> {
        let config = ron::ser::PrettyConfig::new()
            .new_line("\n".to_string())
            .indentor("    ".to_string());
        ron::ser::to_string_pretty(
            &format::EnvelopeSer {
                records: &self.records,
                registry,
            },
            config,
        )
        .map_err(|e| SceneError::Parse(e.to_string()))
    }

    /// Parse + FULLY resolve. Any error here leaves the world untouched (B5).
    pub fn from_ron(text: &str, registry: &TypeRegistry) -> Result<Self, SceneError> {
        // Version gate first (unknown-forward is a typed error, not a parse mystery).
        #[derive(serde::Deserialize)]
        struct Probe {
            format_version: u32,
        }
        let probe: Probe = ron::from_str(text).map_err(|e| SceneError::Parse(e.to_string()))?;
        if probe.format_version > FORMAT_VERSION {
            return Err(SceneError::UnsupportedVersion {
                found: probe.format_version,
                supported: FORMAT_VERSION,
            });
        }
        // format_version < CURRENT: migration chain hooks in here (none yet for v1).

        let mut deserializer =
            ron::Deserializer::from_str(text).map_err(|e| SceneError::Parse(e.to_string()))?;
        let mut records = format::EnvelopeSeed { registry }
            .deserialize(&mut deserializer)
            .map_err(|e| SceneError::Parse(e.to_string()))?;
        records.sort_by_key(|r| r.id.0);
        Ok(Self { records })
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Atomic file I/O
// ---------------------------------------------------------------------------

/// Atomic save: write `path.tmp`, keep one `path.bak` of the previous content, then
/// rename into place. A crash mid-save never corrupts the scene file.
pub fn save_scene_file(world: &World, path: &Path) -> Result<(), SceneError> {
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    let text = capture_scene(world).to_ron(&registry)?;

    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, &text).map_err(SceneError::Io)?;
    if path.exists() {
        let bak = path.with_extension("ron.bak");
        std::fs::copy(path, &bak).map_err(SceneError::Io)?;
    }
    std::fs::rename(&tmp, path).map_err(SceneError::Io)?;
    Ok(())
}

/// Non-destructive load: parse + resolve completely, only then swap the world.
pub fn load_scene_file(world: &mut World, path: &Path) -> Result<usize, SceneError> {
    let text = std::fs::read_to_string(path).map_err(SceneError::Io)?;
    let snapshot = {
        let registry_arc = world.resource::<AppTypeRegistry>().clone();
        let registry = registry_arc.read();
        SceneSnapshot::from_ron(&text, &registry)?
    };
    apply_scene(world, &snapshot, true);
    Ok(snapshot.len())
}

// ---------------------------------------------------------------------------
// Editor integration: actions + plugin (eats the editor_api contract)
// ---------------------------------------------------------------------------

/// Where the current scene lives on disk.
#[derive(Resource, Clone)]
pub struct SceneFile(pub PathBuf);

impl Default for SceneFile {
    fn default() -> Self {
        Self(PathBuf::from("level.ron"))
    }
}

/// Unsaved-changes flag, derived from `Edited` broadcasts.
#[derive(Resource, Default)]
pub struct SceneDirty(pub bool);

/// UI-facing result of a save/load — logging is not user feedback (design bar).
#[derive(Message, Debug)]
pub struct SceneIoFeedback {
    pub message: String,
    pub success: bool,
}

#[derive(Resource, Default)]
struct SceneIoRequests {
    save: bool,
    open: bool,
}

struct ScenesFeature;

impl EditorFeature for ScenesFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("scene", "Scene I/O")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.action(
            ActionDef::new("scene.save", "Save Scene")
                .describe("Write the scene to disk (atomic, with backup)")
                .context("normal")
                .bind("ctrl+s"),
        )
        .action(
            ActionDef::new("transform.drop", "Drop To Surface")
                .describe(
                    "Rest the selection on whatever is beneath it — the floor, a table, \
                     the piece below — instead of leaving it clipping through",
                )
                .context("normal")
                .bind("space d"),
        )
        .action(
            ActionDef::new("scene.open", "Open Scene")
                .describe("Reload the scene from disk")
                .context("normal"),
        )
        .action(
            ActionDef::new("anim.key", "Key Selection")
                .describe("Record where the selection is, at the playhead")
                .context("normal")
                .bind("space k")
                .edit(),
        );
        reg.action(
            ActionDef::new("anim.play", "Play / Pause Timeline")
                .describe("Run the playhead; press again to pause where it is")
                .context("normal")
                .bind("space space"),
        );
        reg.action(
            ActionDef::new("anim.ease", "Cycle Key Easing")
                .describe(
                    "Cycle how the keys at the playhead leave — \
                     linear, in-out, in, out, hold",
                )
                .context("normal")
                .bind("space t c")
                .edit(),
        );
        reg.action(
            ActionDef::new("anim.rewind", "Rewind Timeline")
                .describe("Put the playhead back to the start")
                .context("normal")
                .bind("space 0"),
        );
        reg.action(
            ActionDef::new("level.validate", "Validate Level")
                .describe("Run every registered level rule; problems go to the statusbar and log")
                .context("normal"),
        );
        for validator in level_validation::builtin_level_validators() {
            reg.level_validator(validator);
        }
    }
}

fn collect_io_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut requests: ResMut<SceneIoRequests>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "scene.save" => requests.save = true,
            "scene.open" => requests.open = true,
            _ => {}
        }
    }
}

fn track_dirty(mut edited: MessageReader<Edited>, mut dirty: ResMut<SceneDirty>) {
    if edited.read().next().is_some() {
        dirty.0 = true;
    }
}

fn perform_scene_io(world: &mut World) {
    if world.resource::<SceneIoLock>().0 {
        let requests = std::mem::take(&mut *world.resource_mut::<SceneIoRequests>());
        if requests.save || requests.open {
            world.write_message(SceneIoFeedback {
                message: "scene io locked — finish prefab editing first".into(),
                success: false,
            });
        }
        return;
    }
    let requests = std::mem::take(&mut *world.resource_mut::<SceneIoRequests>());
    if !requests.save && !requests.open {
        return;
    }
    let path = world.resource::<SceneFile>().0.clone();
    if requests.save {
        let feedback = match save_scene_file(world, &path) {
            Ok(()) => {
                world.resource_mut::<SceneDirty>().0 = false;
                SceneIoFeedback {
                    message: format!("saved {}", path.display()),
                    success: true,
                }
            }
            Err(e) => SceneIoFeedback {
                message: format!("save failed: {e}"),
                success: false,
            },
        };
        world.write_message(feedback);
    }
    if requests.open {
        let feedback = match load_scene_file(world, &path) {
            Ok(count) => {
                world.resource_mut::<SceneDirty>().0 = false;
                SceneIoFeedback {
                    message: format!("loaded {count} entities from {}", path.display()),
                    success: true,
                }
            }
            // Non-destructive: the current scene is still intact on failure.
            Err(e) => SceneIoFeedback {
                message: format!("load failed (scene unchanged): {e}"),
                success: false,
            },
        };
        world.write_message(feedback);
    }
}

pub mod anim;
pub mod drop;
pub mod level_validation;
pub mod materials;
pub mod models;
pub mod play;
pub mod session;

#[cfg(test)]
pub(crate) mod tests_support {
    use super::*;
    use editor_core::EditorCorePlugin;

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component)]
    pub struct Health {
        pub current: f32,
        pub max: f32,
    }

    pub struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("test-support", "Test Support")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Health>().component::<Transform>();
        }
    }

    pub fn scene_test_app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_plugins(crate::EditorScenePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.init_resource::<ButtonInput<MouseButton>>();
        app.finish();
        app.update();
        app.world_mut()
            .resource_mut::<editor_core::prelude::EditorState>()
            .active = true;
        app
    }

    pub fn invoke(app: &mut App, action: &str) {
        app.world_mut().write_message(ActionInvoked {
            action: ActionId::new(action.to_string()),
            args: None,
            source: InvocationSource::Test,
        });
        app.update();
    }

    pub fn spawn_test_scene(app: &mut App) -> (SceneId, SceneId) {
        let a = SceneId::random();
        let b = SceneId::random();
        {
            let mut queue = app.world_mut().resource_mut::<EditQueue>();
            queue.0.push(Transaction {
                label: "setup".into(),
                gesture: None,
                ops: vec![
                    Op::Spawn {
                        id: a,
                        components: vec![
                            Box::new(Health {
                                current: 7.5,
                                max: 10.0,
                            })
                            .into_partial_reflect(),
                            Box::new(Transform::from_xyz(1.0, 2.0, 3.0)).into_partial_reflect(),
                        ],
                    },
                    Op::Spawn {
                        id: b,
                        components: vec![],
                    },
                ],
            });
        }
        app.update();
        (a, b)
    }

    pub fn scene_ron(app: &mut App) -> String {
        let world = app.world_mut();
        let registry_arc = world.resource::<AppTypeRegistry>().clone();
        let registry = registry_arc.read();
        capture_scene(world).to_ron(&registry).unwrap()
    }
}

pub struct EditorScenePlugin;

impl Plugin for EditorScenePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SceneFile>()
            .init_resource::<SceneDirty>()
            .init_resource::<SceneIoRequests>()
            .init_resource::<anim::Timeline>()
            .init_resource::<drop::DropRequested>()
            .init_resource::<anim::Playhead>()
            .init_resource::<play::PlayState>()
            .init_resource::<play::PlayRequests>()
            .init_resource::<materials::MaterialLibrary>()
            .init_resource::<materials::MaterialHandles>()
            .init_resource::<models::ModelLibrary>()
            .init_resource::<models::ImportRequested>()
            .init_resource::<models::FlattenRequested>()
            .init_resource::<models::ModelHandles>()
            .init_resource::<models::ProcessedAssets>()
            .init_resource::<level_validation::LevelValidation>()
            .init_resource::<level_validation::ValidationRequests>()
            .init_resource::<session::ReloadRequested>()
            .init_resource::<SceneIoLock>()
            .add_message::<SceneIoFeedback>()
            .add_message::<anim::TimelineEvent>();
        app.add_editor_feature(ScenesFeature);
        app.add_editor_feature(play::PlayFeature);
        app.add_editor_feature(materials::MaterialsFeature);
        app.add_editor_feature(models::ModelsFeature);
        app.add_editor_feature(session::ReloadFeature);
        app.add_systems(Startup, materials::load_library_at_startup);
        app.add_systems(Startup, anim::load_timeline_at_startup);
        app.add_systems(Startup, models::import_at_startup);
        app.add_systems(
            Update,
            (
                (
                    collect_io_actions,
                    track_dirty,
                    play::collect_play_actions,
                    materials::handle_material_actions,
                    models::collect_model_actions,
                    level_validation::collect_validation_requests,
                    session::collect_reload_action,
                    anim::handle_anim_actions,
                    drop::collect_drop_action,
                )
                    .in_set(editor_core::EditorSet::Tools),
                (
                    perform_scene_io,
                    drop::perform_drop,
                    play::perform_play,
                    materials::save_library_on_change,
                    models::perform_import,
                    models::perform_flatten,
                    models::resolve_mesh_refs,
                    models::resolve_mesh_nodes,
                    // AFTER the model resolvers, always: both write
                    // `MeshMaterial3d` on the same entities, and an assigned
                    // material must win over the gltf-authored one.
                    materials::sync_material_refs,
                    level_validation::run_level_validation,
                    session::perform_reload,
                    // Time moves, then what it implies is written. Both sit in
                    // Sync rather than the edit path on purpose: evaluation is
                    // not history (see `anim`), so it must never queue a
                    // transaction.
                    anim::advance_playhead,
                    anim::fire_timeline_events,
                    anim::save_timeline_on_change,
                    anim::evaluate_timeline,
                )
                    .chain()
                    .in_set(editor_core::EditorSet::Sync),
            ),
        );
    }
}

// ---------------------------------------------------------------------------
// Wire format: hand-written serde so component values serialize INLINE through the
// reflect (de)serializers — one coherent document, no RON-in-RON (v1 anti-pattern).
// ---------------------------------------------------------------------------

mod format {
    use super::{FORMAT_VERSION, ResolvedRecord};
    use bevy::reflect::serde::{TypedReflectDeserializer, TypedReflectSerializer};
    use bevy::reflect::{PartialReflect, ReflectFromReflect, TypeRegistry};
    use editor_api::prelude::SceneId;
    use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
    use serde::ser::{Serialize, SerializeSeq, SerializeStruct, Serializer};
    use uuid::Uuid;

    // ---------------- serialize ----------------

    pub(crate) struct EnvelopeSer<'a> {
        pub(crate) records: &'a [ResolvedRecord],
        pub(crate) registry: &'a TypeRegistry,
    }

    impl Serialize for EnvelopeSer<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut st = serializer.serialize_struct("Envelope", 2)?;
            st.serialize_field("format_version", &FORMAT_VERSION)?;
            st.serialize_field(
                "entities",
                &EntitiesSer {
                    records: self.records,
                    registry: self.registry,
                },
            )?;
            st.end()
        }
    }

    struct EntitiesSer<'a> {
        records: &'a [ResolvedRecord],
        registry: &'a TypeRegistry,
    }

    impl Serialize for EntitiesSer<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut seq = serializer.serialize_seq(Some(self.records.len()))?;
            for record in self.records {
                seq.serialize_element(&EntitySer {
                    record,
                    registry: self.registry,
                })?;
            }
            seq.end()
        }
    }

    struct EntitySer<'a> {
        record: &'a ResolvedRecord,
        registry: &'a TypeRegistry,
    }

    impl Serialize for EntitySer<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut st = serializer.serialize_struct("Entity", 3)?;
            st.serialize_field("id", &self.record.id.0)?;
            st.serialize_field("parent", &self.record.parent.map(|p| p.0))?;
            st.serialize_field(
                "components",
                &ComponentsSer {
                    record: self.record,
                    registry: self.registry,
                },
            )?;
            st.end()
        }
    }

    struct ComponentsSer<'a> {
        record: &'a ResolvedRecord,
        registry: &'a TypeRegistry,
    }

    impl Serialize for ComponentsSer<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            // Deterministic order (spike-4 rules): sort by type path.
            let mut pairs: Vec<(&str, &dyn PartialReflect)> = self
                .record
                .components
                .iter()
                .filter_map(|value| {
                    let info = value.get_represented_type_info()?;
                    Some((info.type_path(), value.as_ref() as &dyn PartialReflect))
                })
                .collect();
            pairs.sort_by_key(|(path, _)| *path);
            let mut seq = serializer.serialize_seq(Some(pairs.len()))?;
            for (type_path, value) in pairs {
                seq.serialize_element(&ComponentSer {
                    type_path,
                    value,
                    registry: self.registry,
                })?;
            }
            seq.end()
        }
    }

    struct ComponentSer<'a> {
        type_path: &'a str,
        value: &'a dyn PartialReflect,
        registry: &'a TypeRegistry,
    }

    impl Serialize for ComponentSer<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            // Captured values are DYNAMIC clones; the serializer only finds custom
            // serde (ReflectSerialize — e.g. glam's tuple form) on CONCRETE types.
            // Upgrade via ReflectFromReflect so serialize and deserialize agree on
            // the wire shape (the "Expected identifier" asymmetry).
            let concrete = self
                .registry
                .get_with_type_path(self.type_path)
                .and_then(|reg| reg.data::<ReflectFromReflect>())
                .and_then(|fr| fr.from_reflect(self.value));
            let value: &dyn PartialReflect = concrete
                .as_deref()
                .map(|c| c.as_partial_reflect())
                .unwrap_or(self.value);
            let mut st = serializer.serialize_struct("Component", 2)?;
            st.serialize_field("type_path", self.type_path)?;
            st.serialize_field("value", &TypedReflectSerializer::new(value, self.registry))?;
            st.end()
        }
    }

    // ---------------- deserialize ----------------

    /// RON serves struct-field keys only through `deserialize_identifier`; asking for
    /// a plain `String` there is "Expected identifier". This key type asks correctly.
    struct Ident(String);

    impl<'de> serde::Deserialize<'de> for Ident {
        fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            struct V;
            impl Visitor<'_> for V {
                type Value = Ident;
                fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                    f.write_str("a field identifier")
                }
                fn visit_str<E: de::Error>(self, s: &str) -> Result<Ident, E> {
                    Ok(Ident(s.to_string()))
                }
            }
            d.deserialize_identifier(V)
        }
    }

    pub(crate) struct EnvelopeSeed<'a> {
        pub(crate) registry: &'a TypeRegistry,
    }

    impl<'de> DeserializeSeed<'de> for EnvelopeSeed<'_> {
        type Value = Vec<ResolvedRecord>;
        fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_struct("Envelope", &["format_version", "entities"], self)
        }
    }

    impl<'de> Visitor<'de> for EnvelopeSeed<'_> {
        type Value = Vec<ResolvedRecord>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a scene envelope")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut entities = None;
            while let Some(key) = map.next_key::<Ident>()? {
                match key.0.as_str() {
                    "format_version" => {
                        map.next_value::<u32>()?;
                    }
                    "entities" => {
                        entities = Some(map.next_value_seed(EntitiesSeed {
                            registry: self.registry,
                        })?);
                    }
                    _ => {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
            }
            entities.ok_or_else(|| de::Error::missing_field("entities"))
        }
    }

    struct EntitiesSeed<'a> {
        registry: &'a TypeRegistry,
    }

    impl<'de> DeserializeSeed<'de> for EntitiesSeed<'_> {
        type Value = Vec<ResolvedRecord>;
        fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_seq(self)
        }
    }

    impl<'de> Visitor<'de> for EntitiesSeed<'_> {
        type Value = Vec<ResolvedRecord>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list of entities")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut records = Vec::new();
            while let Some(record) = seq.next_element_seed(EntitySeed {
                registry: self.registry,
            })? {
                records.push(record);
            }
            Ok(records)
        }
    }

    struct EntitySeed<'a> {
        registry: &'a TypeRegistry,
    }

    impl<'de> DeserializeSeed<'de> for EntitySeed<'_> {
        type Value = ResolvedRecord;
        fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_struct("Entity", &["id", "parent", "components"], self)
        }
    }

    impl<'de> Visitor<'de> for EntitySeed<'_> {
        type Value = ResolvedRecord;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an entity record")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut id: Option<Uuid> = None;
            let mut parent: Option<Option<Uuid>> = None;
            let mut components = Vec::new();
            while let Some(key) = map.next_key::<Ident>()? {
                match key.0.as_str() {
                    "id" => id = Some(map.next_value()?),
                    "parent" => parent = Some(map.next_value()?),
                    "components" => {
                        components = map.next_value_seed(ComponentsSeed {
                            registry: self.registry,
                        })?;
                    }
                    _ => {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
            }
            Ok(ResolvedRecord {
                id: SceneId(id.ok_or_else(|| de::Error::missing_field("id"))?),
                parent: parent.flatten().map(SceneId),
                components,
            })
        }
    }

    struct ComponentsSeed<'a> {
        registry: &'a TypeRegistry,
    }

    impl<'de> DeserializeSeed<'de> for ComponentsSeed<'_> {
        type Value = Vec<Box<dyn PartialReflect>>;
        fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_seq(self)
        }
    }

    impl<'de> Visitor<'de> for ComponentsSeed<'_> {
        type Value = Vec<Box<dyn PartialReflect>>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a list of components")
        }
        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut values = Vec::new();
            while let Some(value) = seq.next_element_seed(ComponentSeed {
                registry: self.registry,
            })? {
                values.push(value);
            }
            Ok(values)
        }
    }

    struct ComponentSeed<'a> {
        registry: &'a TypeRegistry,
    }

    impl<'de> DeserializeSeed<'de> for ComponentSeed<'_> {
        type Value = Box<dyn PartialReflect>;
        fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
            d.deserialize_struct("Component", &["type_path", "value"], self)
        }
    }

    impl<'de> Visitor<'de> for ComponentSeed<'_> {
        type Value = Box<dyn PartialReflect>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a component record (type_path before value)")
        }
        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
            let mut registration = None;
            let mut value = None;
            while let Some(key) = map.next_key::<Ident>()? {
                match key.0.as_str() {
                    "type_path" => {
                        let type_path: String = map.next_value()?;
                        registration =
                            Some(self.registry.get_with_type_path(&type_path).ok_or_else(
                                || {
                                    de::Error::custom(format!(
                                        "unknown component type `{type_path}`"
                                    ))
                                },
                            )?);
                    }
                    "value" => {
                        let Some(registration) = registration else {
                            return Err(de::Error::custom(
                                "component `type_path` must precede `value`",
                            ));
                        };
                        value = Some(map.next_value_seed(TypedReflectDeserializer::new(
                            registration,
                            self.registry,
                        ))?);
                    }
                    _ => {
                        map.next_value::<IgnoredAny>()?;
                    }
                }
            }
            value.ok_or_else(|| de::Error::missing_field("value"))
        }
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::EditorCorePlugin;

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component)]
    struct Health {
        current: f32,
        max: f32,
    }

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("test", "Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Health>().component::<Transform>();
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_plugins(EditorScenePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.finish();
        app.update();
        app
    }

    fn spawn_test_scene(app: &mut App) -> (SceneId, SceneId) {
        let a = SceneId::random();
        let b = SceneId::random();
        {
            let mut queue = app.world_mut().resource_mut::<EditQueue>();
            queue.0.push(Transaction {
                label: "setup".into(),
                gesture: None,
                ops: vec![
                    Op::Spawn {
                        id: a,
                        components: vec![
                            Box::new(Health {
                                current: 7.5,
                                max: 10.0,
                            })
                            .into_partial_reflect(),
                            Box::new(Transform::from_xyz(1.0, 2.0, 3.0)).into_partial_reflect(),
                        ],
                    },
                    Op::Spawn {
                        id: b,
                        components: vec![],
                    },
                    Op::Reparent {
                        target: b,
                        parent: Some(a),
                    },
                ],
            });
        }
        app.update();
        (a, b)
    }

    fn scene_ron(app: &mut App) -> String {
        let world = app.world_mut();
        let registry_arc = world.resource::<AppTypeRegistry>().clone();
        let registry = registry_arc.read();
        capture_scene(world).to_ron(&registry).unwrap()
    }

    // B4: save -> load -> save is byte-identical
    #[test]
    fn round_trip_is_byte_identical() {
        let mut app = test_app();
        let (a, _b) = spawn_test_scene(&mut app);
        let first = scene_ron(&mut app);
        assert!(first.contains("format_version: 1"));

        // Load into a FRESH app and re-serialize.
        let mut app2 = test_app();
        {
            let world = app2.world_mut();
            let registry_arc = world.resource::<AppTypeRegistry>().clone();
            let registry = registry_arc.read();
            let snapshot = SceneSnapshot::from_ron(&first, &registry).unwrap();
            drop(registry);
            apply_scene(world, &snapshot, true);
        }
        let second = scene_ron(&mut app2);
        assert_eq!(first, second, "byte-identical round trip");

        // And the loaded world is semantically right.
        let world = app2.world_mut();
        let entity = world.resource::<SceneIndex>().get(&a).unwrap();
        assert_eq!(
            world.get::<Health>(entity),
            Some(&Health {
                current: 7.5,
                max: 10.0
            })
        );
    }

    // B5: corrupt/unknown input leaves the world untouched
    #[test]
    fn bad_loads_are_non_destructive() {
        let mut app = test_app();
        spawn_test_scene(&mut app);
        let before = scene_ron(&mut app);

        let dir = tempfile::tempdir().unwrap();
        let garbage = dir.path().join("level.ron");
        std::fs::write(&garbage, "(format_version: 1, entities: [ THIS IS NOT RON").unwrap();
        assert!(load_scene_file(app.world_mut(), &garbage).is_err());
        assert_eq!(
            scene_ron(&mut app),
            before,
            "corrupt file must not destroy the scene"
        );

        // Unknown component type: fully rejected, still untouched.
        let unknown = dir.path().join("unknown.ron");
        std::fs::write(
            &unknown,
            r#"(format_version: 1, entities: [(id: "6dbb56d1-a3c8-4a5e-9d55-0f7b1b3a0001", components: [(type_path: "no::such::Type", value: ())])])"#,
        )
        .unwrap();
        let err = load_scene_file(app.world_mut(), &unknown).unwrap_err();
        assert!(
            err.to_string().contains("no::such::Type"),
            "error names the type: {err}"
        );
        assert_eq!(scene_ron(&mut app), before);

        // Future version: refused with the typed error.
        let future = dir.path().join("future.ron");
        std::fs::write(&future, "(format_version: 999, entities: [])").unwrap();
        assert!(matches!(
            load_scene_file(app.world_mut(), &future),
            Err(SceneError::UnsupportedVersion { found: 999, .. })
        ));
    }

    // B5: atomic save keeps a .bak of the previous content
    #[test]
    fn save_is_atomic_with_backup() {
        let mut app = test_app();
        spawn_test_scene(&mut app);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("level.ron");

        save_scene_file(app.world(), &path).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();

        // Change the scene, save again: .bak holds the previous save.
        {
            let a = *app
                .world_mut()
                .resource::<SceneIndex>()
                .iter()
                .next()
                .unwrap()
                .0;
            let mut queue = app.world_mut().resource_mut::<EditQueue>();
            queue.0.push(Transaction {
                label: "edit".into(),
                gesture: None,
                ops: vec![Op::Set {
                    target: a,
                    value: Box::new(Transform::from_xyz(9.0, 9.0, 9.0)).into_partial_reflect(),
                }],
            });
        }
        app.update();
        save_scene_file(app.world(), &path).unwrap();

        let bak = std::fs::read_to_string(path.with_extension("ron.bak")).unwrap();
        assert_eq!(bak, first, ".bak preserves the previous save");
        assert_ne!(std::fs::read_to_string(&path).unwrap(), first);
        assert!(!path.with_extension("ron.tmp").exists(), "no temp litter");
    }
}

#[cfg(test)]
mod adoption_tests {
    use super::*;
    use crate::tests_support::*;

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component, Default)]
    struct RuntimeAdded(f32);

    // Owner rule: anything a user INSERTS persists. Adoption extends the save
    // set at runtime, and LOADING a file containing the type re-adopts it —
    // so adopted components survive editor restarts through their files.
    #[test]
    fn adopted_components_persist_and_reload() {
        let mut app = scene_test_app();
        app.register_type::<RuntimeAdded>(); // registry only — NOT feature-registered
        let (a, _) = spawn_test_scene(&mut app);
        let entity = app.world().resource::<SceneIndex>().get(&a).unwrap();
        app.world_mut().entity_mut(entity).insert(RuntimeAdded(7.0));

        // Not in the allow-list: capture drops it.
        let before = scene_ron(&mut app);
        assert!(
            !before.contains("RuntimeAdded"),
            "unadopted type is not captured"
        );

        // Adopt (what the palette insert does) → captured.
        {
            let mut components = app
                .world_mut()
                .resource_mut::<editor_core::edits::EditorComponents>();
            components.adopt(
                std::any::TypeId::of::<RuntimeAdded>(),
                <RuntimeAdded as bevy::reflect::TypePath>::type_path(),
            );
        }
        let saved = scene_ron(&mut app);
        assert!(saved.contains("RuntimeAdded"), "adopted type serializes");

        // Simulate a restart: strip the adoption, load the file → re-adopted.
        {
            let mut components = app
                .world_mut()
                .resource_mut::<editor_core::edits::EditorComponents>();
            components
                .types
                .retain(|r| r.type_id != std::any::TypeId::of::<RuntimeAdded>());
        }
        let registry = app.world().resource::<AppTypeRegistry>().clone();
        let snapshot = SceneSnapshot::from_ron(&saved, &registry.read()).unwrap();
        apply_scene(app.world_mut(), &snapshot, false);
        let adopted = app
            .world()
            .resource::<editor_core::edits::EditorComponents>()
            .contains(std::any::TypeId::of::<RuntimeAdded>());
        assert!(adopted, "loading a file re-adopts its types");
        let roundtrip = scene_ron(&mut app);
        assert!(
            roundtrip.contains("RuntimeAdded"),
            "survives the next save too"
        );
    }
}
