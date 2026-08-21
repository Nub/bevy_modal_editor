//! `editor_prefabs` (spec §6, M4-D4): the prefab is the unit of game-ready.
//!
//! - A **prefab asset** = versioned envelope: header (uuid, name, parameters
//!   later) + a template hierarchy stored in THE scene format (same envelope
//!   discipline, same serializer — one format, two containers).
//! - An **instance** is a reference with deltas: the scene serializes ONE root
//!   record carrying `PrefabInstance(prefab_id)` + `Transform` +
//!   `PrefabOverrides` — NEVER the expanded tree (v1 expanded copies; that
//!   defeats the point). Template entities stamp in at load/spawn as
//!   `PrefabStamped` children, which scene capture excludes by construction.
//! - Stamping happens through the same reflection-apply path as scene load —
//!   regenerate hooks (mesh derivation etc.) fire exactly as they do for
//!   hand-placed entities.

use bevy::ecs::relationship::RelationshipHookMode;
use bevy::prelude::*;
use editor_core::prelude::*;
use editor_scene::{PrefabStamped, SceneSnapshot};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use uuid::Uuid;

pub mod array;
pub mod authoring;
pub mod bake;
pub mod open_mode;
pub mod overrides;
pub mod paint;
pub mod sockets;
pub mod template_mode;
pub use overrides::{StampedFrom, sync_overrides};

pub const PREFAB_FORMAT_VERSION: u32 = 1;

/// Scene-side reference: this entity is an instance of a library prefab.
#[derive(Component, Reflect, Clone, Copy, PartialEq, Debug, Default)]
#[reflect(Component)]
pub struct PrefabInstance(pub Uuid);

/// Per-instance override deltas (M4-D4 carries the SHAPE; D5 gives the
/// revert/apply verbs and inspector treatment). Each patch targets a template
/// entity's component field by reflect path, value carried as RON.
#[derive(Component, Reflect, Clone, PartialEq, Debug, Default)]
#[reflect(Component)]
pub struct PrefabOverrides(pub Vec<OverridePatch>);

#[derive(Reflect, Clone, PartialEq, Debug, Default)]
pub struct OverridePatch {
    /// Template-local entity id (the prefab's internal SceneId, hyphenated UUID
    /// string — SceneId itself is serde-first, not Reflect).
    pub entity: String,
    pub type_path: String,
    pub path: String,
    /// RON-serialized field value.
    pub value: String,
}

/// A prefab asset: header + template hierarchy.
pub struct PrefabDef {
    pub id: Uuid,
    pub name: String,
    /// Kit membership (D10): pieces sharing a kit are expected to mate —
    /// coherence checks run per kit ("wall-kit", "pipes").
    pub kit: Option<String>,
    pub template: SceneSnapshot,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct PrefabHeader {
    format_version: u32,
    id: Uuid,
    name: String,
    #[serde(default)]
    kit: String,
    /// The template, in THE scene envelope format (nested document).
    template: String,
}

impl Default for PrefabHeader {
    fn default() -> Self {
        Self {
            format_version: PREFAB_FORMAT_VERSION,
            id: Uuid::nil(),
            name: String::new(),
            kit: String::new(),
            template: String::new(),
        }
    }
}

#[derive(Debug)]
pub enum PrefabError {
    Io(std::io::Error),
    Format(String),
    FutureVersion { found: u32, supported: u32 },
}

impl std::fmt::Display for PrefabError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "prefab io: {e}"),
            Self::Format(e) => write!(f, "prefab format: {e}"),
            Self::FutureVersion { found, supported } => write!(
                f,
                "prefab format {found} newer than supported {supported} — upgrade the editor"
            ),
        }
    }
}
impl std::error::Error for PrefabError {}

impl PrefabDef {
    pub fn save(
        &self,
        path: &Path,
        registry: &bevy::reflect::TypeRegistry,
    ) -> Result<(), PrefabError> {
        let header = PrefabHeader {
            format_version: PREFAB_FORMAT_VERSION,
            id: self.id,
            name: self.name.clone(),
            kit: self.kit.clone().unwrap_or_default(),
            template: self
                .template
                .to_ron(registry)
                .map_err(|e| PrefabError::Format(e.to_string()))?,
        };
        let text = ron::ser::to_string_pretty(&header, ron::ser::PrettyConfig::default())
            .map_err(|e| PrefabError::Format(e.to_string()))?;
        let tmp = path.with_extension("ron.tmp");
        std::fs::write(&tmp, &text).map_err(PrefabError::Io)?;
        if path.exists() {
            let bak = path.with_extension("ron.bak");
            let _ = std::fs::copy(path, bak);
        }
        std::fs::rename(&tmp, path).map_err(PrefabError::Io)?;
        Ok(())
    }

    pub fn load(path: &Path, registry: &bevy::reflect::TypeRegistry) -> Result<Self, PrefabError> {
        let text = std::fs::read_to_string(path).map_err(PrefabError::Io)?;
        let header: PrefabHeader =
            ron::from_str(&text).map_err(|e| PrefabError::Format(e.to_string()))?;
        if header.format_version > PREFAB_FORMAT_VERSION {
            return Err(PrefabError::FutureVersion {
                found: header.format_version,
                supported: PREFAB_FORMAT_VERSION,
            });
        }
        let template = SceneSnapshot::from_ron(&header.template, registry)
            .map_err(|e| PrefabError::Format(e.to_string()))?;
        Ok(Self {
            id: header.id,
            name: header.name,
            kit: (!header.kit.is_empty()).then_some(header.kit),
            template,
        })
    }
}

/// Loaded prefabs by id. `generation` bumps on any library change — instances
/// restamp when it moves (the propagation trigger).
#[derive(Resource, Default)]
pub struct PrefabLibrary {
    pub prefabs: HashMap<Uuid, PrefabDef>,
    pub generation: u64,
}

impl PrefabDef {
    /// Placeholder hook (bake staleness etc. later).
    pub fn generation_note(&mut self) {}
}

fn socket_kind_components(position: Vec3) -> Vec<Box<dyn bevy::reflect::PartialReflect>> {
    vec![
        Box::new(sockets::Socket::default()).into_partial_reflect(),
        Box::new(Transform::from_translation(position)).into_partial_reflect(),
        Box::new(Name::new("Socket")).into_partial_reflect(),
    ]
}

/// Concrete `PrefabInstance` off a template record value (values may be
/// DYNAMIC structs fresh off the RON deserializer).
fn reflect_instance(value: &dyn bevy::reflect::PartialReflect) -> Option<PrefabInstance> {
    let is_instance = value
        .get_represented_type_info()
        .is_some_and(|i| i.type_path() == <PrefabInstance as bevy::reflect::TypePath>::type_path());
    if !is_instance {
        return None;
    }
    <PrefabInstance as bevy::reflect::FromReflect>::from_reflect(value)
}

/// Prefabs directly referenced by a template (nested instance records).
pub fn instances_in_template(def: &PrefabDef) -> Vec<Uuid> {
    def.template
        .records()
        .flat_map(|(_, _, components)| {
            components
                .iter()
                .filter_map(|c| reflect_instance(c.as_partial_reflect()).map(|i| i.0))
        })
        .collect()
}

/// Transitive nesting check: does `candidate`'s template — through any chain of
/// nested prefabs — reference `target`? THE cycle gate (D6): adding an instance
/// of `candidate` inside `target` is legal iff this is false.
pub fn closure_contains(library: &PrefabLibrary, candidate: Uuid, target: Uuid) -> bool {
    let mut stack = vec![candidate];
    let mut visited = std::collections::HashSet::new();
    while let Some(current) = stack.pop() {
        if current == target {
            return true;
        }
        if !visited.insert(current) {
            continue;
        }
        if let Some(def) = library.prefabs.get(&current) {
            stack.extend(instances_in_template(def));
        }
    }
    false
}

/// Stamp a prefab's template under an instance root: fresh runtime SceneIds
/// (mapped from template-local ids), `PrefabStamped` markers (capture-excluded),
/// components applied through the same reflection path as scene load.
pub fn stamp_prefab(world: &mut World, prefab_id: Uuid, root: Entity) {
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();

    // Collect the template (clone values out so the library borrow ends).
    let records: Vec<(
        SceneId,
        Option<SceneId>,
        Vec<Box<dyn bevy::reflect::PartialReflect>>,
    )> = {
        let library = world.resource::<PrefabLibrary>();
        let Some(prefab) = library.prefabs.get(&prefab_id) else {
            return;
        };
        prefab
            .template
            .records()
            .map(|(id, parent, components)| {
                (
                    id,
                    parent,
                    components.iter().map(|c| c.to_dynamic()).collect(),
                )
            })
            .collect()
    };

    let root_scene_id = world.get::<SceneId>(root).copied().unwrap_or_default();
    let patches: Vec<OverridePatch> = world
        .get::<PrefabOverrides>(root)
        .map(|o| o.0.clone())
        .unwrap_or_default();

    let mut spawned: HashMap<SceneId, Entity> = HashMap::new();
    for (template_id, _, components) in &records {
        let entity = world
            .spawn((
                SceneId::random(),
                PrefabStamped,
                StampedFrom {
                    instance_root: root_scene_id,
                    template_id: *template_id,
                },
            ))
            .id();
        for value in components {
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
        // Overrides re-apply OVER the template (per-field patches).
        for patch in patches
            .iter()
            .filter(|p| p.entity == template_id.0.to_string())
        {
            let Some(registration) = registry.get_with_type_path(&patch.type_path) else {
                continue;
            };
            let Some(reflect_component) =
                registration.data::<bevy::ecs::reflect::ReflectComponent>()
            else {
                continue;
            };
            let Some(current) = world
                .get_entity(entity)
                .ok()
                .and_then(|e| reflect_component.reflect(e))
            else {
                continue;
            };
            let mut dynamic = current.as_partial_reflect().to_dynamic();
            if overrides::apply_patch_value(&registry, dynamic.as_mut(), &patch.path, &patch.value)
                && let Ok(mut entity_mut) = world.get_entity_mut(entity)
            {
                reflect_component.apply_or_insert_mapped(
                    &mut entity_mut,
                    dynamic.as_ref(),
                    &registry,
                    &mut (),
                    RelationshipHookMode::Run,
                );
            }
        }
        spawned.insert(*template_id, entity);
    }
    // Parent wiring: template-internal parents, roots under the instance root.
    for (template_id, parent, _) in &records {
        let Some(&child) = spawned.get(template_id) else {
            continue;
        };
        let parent_entity = parent
            .and_then(|p| spawned.get(&p).copied())
            .unwrap_or(root);
        world.entity_mut(child).insert(ChildOf(parent_entity));
    }
}

/// Instances stamp when they appear (spawn, scene load, undo respawn) and
/// re-stamp is a despawn-children + stamp (source-edit propagation rides this).
pub fn stamp_new_instances(world: &mut World) {
    let pending: Vec<(Entity, Uuid)> = {
        let mut query = world.query_filtered::<(Entity, &PrefabInstance), Without<Stamped>>();
        query.iter(world).map(|(e, p)| (e, p.0)).collect()
    };
    for (root, prefab_id) in pending {
        world.entity_mut(root).insert(Stamped);
        stamp_prefab(world, prefab_id, root);
    }
}

/// Marks instance roots whose children exist.
#[derive(Component, Default, Clone)]
pub struct Stamped;

pub struct EditorPrefabsPlugin;

impl Plugin for EditorPrefabsPlugin {
    fn build(&self, app: &mut App) {
        // A socket is an authoring HANDLE, not a member of the shape: it stays
        // clickable through the seal that makes a prefab select as a unit.
        // Everything built on "select a socket" is unreachable by mouse
        // otherwise, on exactly the pieces that can use it.
        app.add_observer(
            |add: On<bevy::ecs::lifecycle::Add, sockets::Socket>, mut commands: Commands| {
                commands
                    .entity(add.entity)
                    .insert(editor_core::selection::SelectionHandle);
            },
        );
        app.init_resource::<PrefabLibrary>();
        app.init_resource::<overrides::OverrideCursor>();
        app.init_resource::<authoring::PrefabRequests>();
        app.init_resource::<authoring::GroupPrompt>();
        app.init_resource::<authoring::ChainEntry>();
        app.init_resource::<authoring::GroupCommit>();
        app.init_resource::<authoring::PendingGroupSelect>();
        app.init_resource::<open_mode::OpenInstance>();
        // Open-mode gates scene io through this; idempotent if the scene
        // plugin already initialized it (it always does in the real app).
        app.init_resource::<editor_scene::SceneIoLock>();
        app.init_resource::<authoring::LastRestampedGeneration>();
        app.init_resource::<bake::BakeRequests>();
        app.init_resource::<paint::PaintState>();
        app.init_resource::<paint::PaintRequests>();
        // The feedback channel belongs to editor_scene, and a headless prefab
        // test world does not add that plugin — a system taking a writer for an
        // unregistered message fails param validation and takes the app down.
        app.add_message::<editor_scene::SceneIoFeedback>();
        app.init_resource::<template_mode::TemplateEdit>();
        app.init_resource::<template_mode::TemplateRequests>();
        app.init_resource::<bake::BakeDir>();
        app.init_resource::<bake::LastBakeCheck>();
        app.add_editor_feature(PrefabsFeature);
        app.add_systems(Startup, authoring::load_prefab_library);
        // BEFORE the conventions: escape layering reads PRE-press mode/panel state.
        app.add_systems(
            Update,
            (
                authoring::collect_prefab_actions,
                paint::collect_paint_actions,
                template_mode::collect_template_actions,
                template_mode::guard_scene_io_while_editing_template,
            )
                .before(editor_core::resolver::apply_action_conventions)
                .in_set(editor_core::EditorSet::Tools),
        );
        app.add_systems(
            Update,
            (
                template_mode::perform_template_actions,
                authoring::perform_prefab_actions,
                open_mode::maintain_open_instance,
                authoring::restamp_on_library_change,
                paint::perform_paint_actions,
                paint::paint_click,
                bake::perform_bake,
                bake::watch_bake_staleness,
                bake::headless_bake_mode,
                stamp_new_instances,
                authoring::select_grouped,
                overrides::sync_overrides,
                open_mode::seal_closed_instances,
                authoring::snap_during_drag,
                authoring::pin_pivot_to_selected_socket,
                template_mode::hold_template_layer,
            )
                .chain()
                .in_set(editor_core::EditorSet::Sync),
        );
    }
}

struct PrefabsFeature;

impl EditorFeature for PrefabsFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("prefabs", "Prefabs")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        // The instance root's serialized shape: {prefab_id, transform, overrides}.
        reg.context(paint::PAINT_CONTEXT);
        reg.context(crate::sockets::SOCKET_CONTEXT);
        reg.context(editor_api::prelude::ContextId::new_static("template"));
        // An instance of a prefab IS that prefab, whatever else it carries.
        reg.identity::<PrefabInstance>(editor_api::identity::priority::PREFAB, "", "same prefab");
        reg.component::<PrefabInstance>()
            .component::<PrefabOverrides>()
            .component::<sockets::Socket>()
            .entity_kind(editor_api::prelude::EntityKindDef {
                id: editor_api::prelude::EntityKindId::new_static("prefab.socket"),
                display_name: "Socket",
                components: socket_kind_components,
            })
            .action(
                ActionDef::new("prefab.group", "Group Into Prefab")
                    .describe("Name the selection and replace it with a reusable prefab instance")
                    .context("normal")
                    .bind("g"),
            )
            .action(
                ActionDef::new("prefab.edit-template", "Edit Prefab (its own scene)")
                    .describe(
                        "Open the PREFAB itself at its own origin — changes reach every \
                         instance. Enter edits THIS instance instead",
                    )
                    .context("normal")
                    .bind("space e"),
            )
            .action(
                ActionDef::new("prefab.close-template", "Back To The Level")
                    .describe("Save the prefab and return to the level")
                    .context("template")
                    .bind("escape"),
            )
            .action(
                ActionDef::new("prefab.open", "Open Prefab Instance")
                    .describe("Edit the selected instance in place — Escape closes and saves")
                    .context("normal")
                    .bind("enter"),
            )
            .action(
                ActionDef::new("prefab.revert-overrides", "Revert Prefab Overrides")
                    .describe("Reset the selected instance to its prefab source")
                    .context("normal"),
            )
            .action(
                ActionDef::new("prefab.apply-to-prefab", "Apply Overrides To Prefab")
                    .describe("Fold this instance's changes into the prefab for everyone")
                    .context("normal"),
            )
            .action(
                ActionDef::new("prefab.make-variant", "Make Prefab Variant")
                    .describe("New prefab inheriting the base — this instance's overrides become its identity")
                    .context("normal"),
            )
            .action(
                ActionDef::new("prefab.repeat", "Repeat Piece")
                    .describe("Chain another instance of the selected piece at the armed socket")
                    .context("socket")
                    .bind("o"),
            )
            .action(
                ActionDef::new("prefab.fill", "Fill Run")
                    .describe("Chain N pieces at once — `o` repeated, in one undoable step")
                    .context("normal")
                    .bind("shift+o"),
            )
            .action(
                ActionDef::new("transform.array-x", "Array along X")
                    .describe(
                        "Lay a run of copies along world X — the prompt takes the COUNT, \
                         and a negative one runs the other way. Spacing is the selection's \
                         own width, so pieces tile flush. array repeat run tile step",
                    )
                    .context("normal")
                    .bind("space x x")
                    .edit(),
            )
            .action(
                ActionDef::new("transform.array-y", "Array along Y")
                    .describe("Lay a run of copies along world Y, spaced by the selection's own height")
                    .context("normal")
                    .bind("space x y")
                    .edit(),
            )
            .action(
                ActionDef::new("transform.array-z", "Array along Z")
                    .describe("Lay a run of copies along world Z, spaced by the selection's own depth")
                    .context("normal")
                    .bind("space x z")
                    .edit(),
            )
            .action(
                ActionDef::new("prefab.paint", "Paint With Piece")
                    .describe("Click a polyline; the selected piece chains along it, corners resolve from the kit")
                    .context("normal"),
            )
            .action(
                ActionDef::new("paint.exit", "Exit Paint")
                    .context("paint")
                    .bind("escape")
                    .hidden(),
            )
            .action(
                ActionDef::new("prefab.set-kit", "Set Prefab Kit")
                    .describe("Tag the selected piece's prefab as part of a named kit")
                    .context("normal"),
            )
            // Socket placement helpers: mating needs the socket exactly ON the
            // surface and aimed out of it. Drag it roughly into place, then
            // snap — the feature is chosen by where you already put it.
            .action(
                ActionDef::new("socket.snap-face", "Snap Socket To Face")
                    .describe("Centre the selected socket on the nearest face, aiming outward")
                    .context("normal")
                    .bind("space s f"),
            )
            .action(
                ActionDef::new("socket.snap-edge", "Snap Socket To Edge")
                    .describe("Put the selected socket on the nearest edge, bisecting both faces")
                    .context("normal")
                    .bind("space s e"),
            )
            .action(
                ActionDef::new("socket.snap-corner", "Snap Socket To Corner")
                    .describe("Put the selected socket on the nearest corner, bisecting all three")
                    .context("normal")
                    .bind("space s c"),
            )
            .action(
                ActionDef::new("socket.arm", "Socket Mode")
                    .describe(
                        "Work from a socket: Tab picks which one, i places a piece there, \
                         o chains another of the same",
                    )
                    .context("normal")
                    .bind("o"),
            )
            .action(
                ActionDef::new("socket.insert", "Socket: Place Next Piece")
                    .describe("Place another piece on the armed socket and move along to its free end")
                    .context("socket")
                    .bind("i"),
            )
            .action(
                ActionDef::new("socket.exit", "Socket: Done")
                    .describe("Leave socket mode")
                    .context("socket")
                    .bind("escape"),
            )
            .action(
                ActionDef::new("socket.next", "Socket: Next Socket")
                    .describe(
                        "Arm the next socket on the selected piece — the armed socket is \
                         where `i` places the next piece and where `o` chains from",
                    )
                    .context("socket")
                    .context("normal")
                    .bind("tab")
                    .bind("space s n"),
            )
            // Generate a whole mating set at once. Named for the LAYOUT each
            // produces — a run, a grid, a stack — because that is the choice
            // being made, not "how many faces".
            .action(
                ActionDef::new("socket.generate-ends", "Sockets: Ends (run)")
                    .describe("A socket on each ±X face — a piece that chains end to end")
                    .context("normal")
                    .bind("space s 2"),
            )
            .action(
                ActionDef::new("socket.generate-sides", "Sockets: Sides (grid)")
                    .describe("Sockets on ±X and ±Z — a piece that tiles in the plane")
                    .context("normal")
                    .bind("space s 4"),
            )
            .action(
                ActionDef::new("socket.generate-all", "Sockets: All Faces")
                    .describe("A socket on all six faces, ±Y included")
                    .context("normal")
                    .bind("space s 6"),
            )
            // Direction control: the OUT socket is whichever you select when
            // you chain; this pins the IN end, so an asymmetric piece arrives
            // the way round you meant.
            .action(
                ActionDef::new("chain.set-in", "Chain: Enter By This Socket")
                    .describe(
                        "Pin the selected socket as the end the NEXT chained piece mates by",
                    )
                    .context("normal")
                    .bind("space c i"),
            )
            .action(
                ActionDef::new("prefab.bake", "Bake Now")
                    .describe("Derive all registered bake artifacts (colliders, LODs) for the prefab library")
                    .context("normal"),
            )
            .action(
                ActionDef::new("prefab.flatten", "Flatten Prefab Hierarchy")
                    .describe("While open: every member becomes a direct child of the root, world pose kept")
                    .context("normal"),
            );
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use editor_scene::{capture_scene, snapshot_from_parts};

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component)]
    pub(crate) struct Payload(pub f32);

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("prefab-test", "Prefab Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Payload>()
                .component::<Transform>()
                .component::<Name>()
                .baker(editor_api::bake::BakerDef {
                    id: editor_api::prelude::BakerId::new_static("test.digest"),
                    name: "Template Digest",
                    version: 1,
                    bake: |cx| {
                        Ok(Some(
                            format!(
                                "(name: {:?}, digest: {:?})",
                                cx.prefab_name,
                                blake3::hash(cx.template_ron.as_bytes()).to_hex().as_str()
                            )
                            .into_bytes(),
                        ))
                    },
                });
        }
    }

    pub(crate) fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(editor_core::EditorCorePlugin);
        // Real GlobalTransforms — socket mating math reads world frames.
        app.add_plugins(bevy::transform::TransformPlugin);
        app.add_plugins(EditorPrefabsPlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>();
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;
        app
    }

    pub(crate) fn barrel_prefab() -> PrefabDef {
        let child = SceneId::random();
        PrefabDef {
            kit: None,
            id: Uuid::new_v4(),
            name: "Barrel".into(),
            template: snapshot_from_parts(vec![(
                child,
                None,
                vec![
                    Box::new(Payload(7.0)).into_partial_reflect(),
                    Box::new(Transform::from_xyz(0.0, 0.5, 0.0)).into_partial_reflect(),
                ],
            )]),
        }
    }

    // D4: instance stamps children (capture-excluded); the scene serializes the
    // ROOT record only — never the expanded tree; envelope round-trips.
    #[test]
    fn instances_never_expand() {
        let mut app = test_app();
        let prefab = crate::tests::barrel_prefab();
        let prefab_id = prefab.id;

        // Envelope round-trip through disk.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("barrel.prefab.ron");
        {
            let world = app.world();
            let registry = world.resource::<AppTypeRegistry>().clone();
            prefab.save(&path, &registry.read()).unwrap();
            let loaded = PrefabDef::load(&path, &registry.read()).unwrap();
            assert_eq!(loaded.id, prefab_id);
            assert_eq!(loaded.name, "Barrel");
        }
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab_id, prefab);

        // Spawn an instance through the EDIT path (undoable like everything).
        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Place Barrel".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                        Box::new(Transform::from_xyz(3.0, 0.0, 0.0)).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update(); // stamp system pass

        // Stamped child exists, carries the payload, parents under the root.
        let world = app.world_mut();
        let stamped: Vec<(Entity, f32)> = world
            .query_filtered::<(Entity, &Payload), With<PrefabStamped>>()
            .iter(world)
            .map(|(e, p)| (e, p.0))
            .collect();
        assert_eq!(stamped.len(), 1, "template stamped");
        assert_eq!(stamped[0].1, 7.0);
        let root = world.resource::<SceneIndex>().get(&root_id).unwrap();
        assert_eq!(
            world.get::<ChildOf>(stamped[0].0).map(|c| c.parent()),
            Some(root),
            "stamped under instance root"
        );

        // THE gate: capture serializes the root record ONLY.
        let snapshot = capture_scene(world);
        let records: Vec<_> = snapshot.records().collect();
        assert_eq!(records.len(), 1, "never the expanded tree");
        assert_eq!(records[0].0, root_id);
    }

    // The game's regenerate pattern (spec §5): render state derives from an
    // `Add` observer on the semantic component. Stamping inserts via reflection —
    // this pins that reflected inserts FIRE those observers (a silent miss here
    // renders nothing, loudly reported by the owner otherwise).
    #[test]
    fn stamping_fires_regenerate_observers() {
        #[derive(Component)]
        struct Derived;

        let mut app = test_app();
        app.add_observer(
            |add: On<bevy::ecs::lifecycle::Add, Payload>, mut commands: Commands| {
                commands.entity(add.entity).insert(Derived);
            },
        );
        let prefab = crate::tests::barrel_prefab();
        let prefab_id = prefab.id;
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab_id, prefab);

        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: SceneId::random(),
                    components: vec![
                        Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();

        let world = app.world_mut();
        let stamped: Vec<(bool, bool)> = world
            .query_filtered::<(Has<Derived>, Has<Payload>), With<PrefabStamped>>()
            .iter(world)
            .collect();
        assert_eq!(stamped.len(), 1);
        assert!(stamped[0].1, "payload stamped");
        assert!(stamped[0].0, "Add observer fired for the reflected insert");
    }

    // D6 nesting: a template referencing another prefab stamps RECURSIVELY —
    // the nested root is a stamped reference, its subtree stamps beneath it,
    // and scene capture still serializes exactly one record.
    #[test]
    fn nested_instances_stamp_recursively() {
        let mut app = test_app();
        let barrel = barrel_prefab();
        let barrel_id = barrel.id;
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(barrel_id, barrel);

        let crate_id = Uuid::new_v4();
        let crate_def = PrefabDef {
            kit: None,
            id: crate_id,
            name: "Crate".into(),
            template: snapshot_from_parts(vec![
                (
                    SceneId::random(),
                    None,
                    vec![Box::new(Payload(1.0)).into_partial_reflect()],
                ),
                (
                    SceneId::random(),
                    None,
                    vec![
                        Box::new(PrefabInstance(barrel_id)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                        Box::new(Transform::from_xyz(1.0, 0.0, 0.0)).into_partial_reflect(),
                    ],
                ),
            ]),
        };
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(crate_id, crate_def);

        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Place Crate".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(crate_id)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        for _ in 0..4 {
            app.update(); // outer stamp, then the nested instance stamps next pass
        }

        let world = app.world_mut();
        // The nested barrel root exists as a stamped reference…
        let nested_roots: Vec<Entity> = world
            .query_filtered::<Entity, (With<PrefabInstance>, With<PrefabStamped>)>()
            .iter(world)
            .collect();
        assert_eq!(nested_roots.len(), 1, "nested instance root stamped");
        // …and the barrel's OWN payload stamped beneath it (recursion).
        let payloads: Vec<f32> = world
            .query_filtered::<&Payload, With<PrefabStamped>>()
            .iter(world)
            .map(|p| p.0)
            .collect();
        assert!(
            payloads.contains(&7.0),
            "barrel payload stamped through nesting: {payloads:?}"
        );
        assert!(payloads.contains(&1.0), "crate's own record stamped");
        // Capture: still ONE record — nothing expands.
        assert_eq!(capture_scene(world).records().count(), 1);
    }

    // D6 cycle chains: transitive closure detection, and placement-while-open
    // refuses an instance that would close a chain (typed at author time).
    #[test]
    fn cycle_chains_refused() {
        let mut app = test_app();
        // Chain: A contains B, B contains C.
        let (a, b, c) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
        let make = |name: &str, id: Uuid, nested: Option<Uuid>| PrefabDef {
            kit: None,
            id,
            name: name.into(),
            template: snapshot_from_parts(vec![(
                SceneId::random(),
                None,
                match nested {
                    Some(n) => vec![
                        Box::new(PrefabInstance(n)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                    ],
                    None => vec![Box::new(Payload(3.0)).into_partial_reflect()],
                },
            )]),
        };
        {
            let mut library = app.world_mut().resource_mut::<PrefabLibrary>();
            library.prefabs.insert(a, make("A", a, Some(b)));
            library.prefabs.insert(b, make("B", b, Some(c)));
            library.prefabs.insert(c, make("C", c, None));
        }
        {
            let library = app.world().resource::<PrefabLibrary>();
            assert!(closure_contains(library, a, c), "A → B → C detected");
            assert!(!closure_contains(library, c, a), "no reverse edge");
        }

        // Open an instance of C, then place an instance of A: adopting it
        // would create C ∋ A ∋ B ∋ C — refused, left at scene root.
        let c_root = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Place C".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: c_root,
                    components: vec![
                        Box::new(PrefabInstance(c)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        {
            let world = app.world_mut();
            let entity = world.resource::<SceneIndex>().get(&c_root).unwrap();
            world.entity_mut(entity).insert(Selected);
        }
        invoke(&mut app, "prefab.open");
        assert!(
            app.world()
                .resource::<open_mode::OpenInstance>()
                .0
                .is_some()
        );

        let a_root = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Place A".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: a_root,
                    components: vec![
                        Box::new(PrefabInstance(a)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        let world = app.world_mut();
        let a_entity = world.resource::<SceneIndex>().get(&a_root).unwrap();
        assert!(
            world.get::<ChildOf>(a_entity).is_none(),
            "cycle-closing instance NOT adopted into the open prefab"
        );
        invoke(&mut app, "prefab.open"); // close cleanly
        cleanup_prefab_file("c");
    }

    // D6 variants: variant = prefab whose template references the base with the
    // captured deltas; base edits propagate to variant instances.
    #[test]
    fn variant_inherits_from_base() {
        let mut app = test_app();
        let barrel = barrel_prefab();
        let barrel_id = barrel.id;
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(barrel_id, barrel);

        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(barrel_id)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                        Box::new(Transform::from_xyz(5.0, 0.0, 0.0)).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        {
            let world = app.world_mut();
            let entity = world.resource::<SceneIndex>().get(&root_id).unwrap();
            world.entity_mut(entity).insert(Selected);
        }
        // Variant via the prompt path.
        app.world_mut()
            .resource_mut::<authoring::GroupPrompt>()
            .purpose = authoring::PromptPurpose::Variant;
        app.world_mut().resource_mut::<authoring::GroupCommit>().0 = Some("RedBarrel".into());
        for _ in 0..4 {
            app.update();
        }

        let world = app.world_mut();
        // Original instance replaced by a variant instance at the same spot.
        assert!(world.resource::<SceneIndex>().get(&root_id).is_none());
        let variant_id = {
            let library = world.resource::<PrefabLibrary>();
            let def = library
                .prefabs
                .values()
                .find(|p| p.name == "RedBarrel")
                .unwrap();
            assert_eq!(
                def.template.records().count(),
                1,
                "variant template = one reference"
            );
            assert_eq!(instances_in_template(def), vec![barrel_id]);
            def.id
        };
        let roots: Vec<(Entity, &PrefabInstance, &Transform)> = world
            .query_filtered::<(Entity, &PrefabInstance, &Transform), Without<PrefabStamped>>()
            .iter(world)
            .collect();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].1.0, variant_id);
        assert_eq!(roots[0].2.translation, Vec3::new(5.0, 0.0, 0.0));

        // Inheritance: edit the BASE template; the variant instance follows.
        {
            let mut library = app.world_mut().resource_mut::<PrefabLibrary>();
            let base = library.prefabs.get_mut(&barrel_id).unwrap();
            let records: Vec<_> = base
                .template
                .records()
                .map(|(id, parent, c)| {
                    let values: Vec<Box<dyn bevy::reflect::PartialReflect>> = c
                        .iter()
                        .map(|v| {
                            if v.get_represented_type_info()
                                .is_some_and(|i| i.type_path().ends_with("Payload"))
                            {
                                Box::new(Payload(99.0)).into_partial_reflect()
                            } else {
                                v.to_dynamic()
                            }
                        })
                        .collect();
                    (id, parent, values)
                })
                .collect();
            base.template = snapshot_from_parts(records);
            library.generation += 1;
        }
        for _ in 0..4 {
            app.update();
        }
        let world = app.world_mut();
        let payloads: Vec<f32> = world
            .query_filtered::<&Payload, With<PrefabStamped>>()
            .iter(world)
            .map(|p| p.0)
            .collect();
        assert_eq!(
            payloads,
            vec![99.0],
            "base edit propagated through the variant chain"
        );
        cleanup_prefab_file("redbarrel");
    }

    // Owner ask: imported hierarchies can be flattened inside an open instance —
    // members become direct root children, world pose preserved, undoable.
    #[test]
    fn flatten_inside_open_instance() {
        let mut app = test_app();
        // Template: parent (at x=2) → child (local x=1, world x=3).
        let parent_id = SceneId::random();
        let child_id = SceneId::random();
        let prefab_id = Uuid::new_v4();
        let def = PrefabDef {
            kit: None,
            id: prefab_id,
            name: "DeepThing".into(),
            template: snapshot_from_parts(vec![
                (
                    parent_id,
                    None,
                    vec![
                        Box::new(Payload(1.0)).into_partial_reflect(),
                        Box::new(Transform::from_xyz(2.0, 0.0, 0.0)).into_partial_reflect(),
                    ],
                ),
                (
                    child_id,
                    Some(parent_id),
                    vec![
                        Box::new(Payload(2.0)).into_partial_reflect(),
                        Box::new(Transform::from_xyz(1.0, 0.0, 0.0)).into_partial_reflect(),
                    ],
                ),
            ]),
        };
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab_id, def);

        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        {
            let world = app.world_mut();
            let entity = world.resource::<SceneIndex>().get(&root_id).unwrap();
            world.entity_mut(entity).insert(Selected);
        }
        invoke(&mut app, "prefab.open");
        invoke(&mut app, "prefab.flatten");
        app.update();

        {
            let world = app.world_mut();
            let root = world.resource::<SceneIndex>().get(&root_id).unwrap();
            let stamped: Vec<(Entity, f32, &Transform, &ChildOf)> = world
                .query_filtered::<(Entity, &Payload, &Transform, &ChildOf), With<PrefabStamped>>()
                .iter(world)
                .map(|(e, p, t, c)| (e, p.0, t, c))
                .collect();
            assert_eq!(stamped.len(), 2);
            for (_, payload, transform, child_of) in &stamped {
                assert_eq!(child_of.parent(), root, "all members direct under root");
                if *payload == 2.0 {
                    assert_eq!(
                        transform.translation,
                        Vec3::new(3.0, 0.0, 0.0),
                        "world pose preserved through the reparent"
                    );
                }
            }
        }
        // Close: the FLAT structure becomes the template.
        invoke(&mut app, "prefab.open");
        app.update();
        let world = app.world_mut();
        let library = world.resource::<PrefabLibrary>();
        let def = library.prefabs.get(&prefab_id).unwrap();
        assert!(
            def.template
                .records()
                .all(|(_, parent, _)| parent.is_none()),
            "template records all top-level after flatten"
        );
        cleanup_prefab_file("deepthing");
    }

    // Legacy templates (pre-rebase flow) migrate to the centered convention on
    // load: X/Z centroid moves to the root, member HEIGHTS are preserved.
    #[test]
    fn legacy_templates_center_on_load() {
        let template = snapshot_from_parts(vec![
            (
                SceneId::random(),
                None,
                vec![Box::new(Transform::from_xyz(2.0, 0.5, -1.0)).into_partial_reflect()],
            ),
            (
                SceneId::random(),
                None,
                vec![Box::new(Transform::from_xyz(4.0, 1.5, 3.0)).into_partial_reflect()],
            ),
        ]);
        let centered = authoring::center_template(&template).expect("off-center → migrates");
        let translations: Vec<Vec3> = centered
            .records()
            .filter_map(|(_, _, c)| {
                c.iter().find_map(|v| {
                    <Transform as bevy::reflect::FromReflect>::from_reflect(v.as_partial_reflect())
                })
            })
            .map(|t| t.translation)
            .collect();
        // Records are UUID-sorted — order is nondeterministic; compare as a set.
        let mut sorted = translations.clone();
        sorted.sort_by(|a, b| a.x.total_cmp(&b.x));
        assert_eq!(
            sorted,
            vec![Vec3::new(-1.0, 0.5, -2.0), Vec3::new(1.0, 1.5, 2.0)]
        );
        // Already-centered templates are left alone (no save churn).
        assert!(authoring::center_template(&centered).is_none());
    }

    // Handlers match on action STRINGS; registration is what makes an action
    // real (palette, keys, which-key). A handler without a registered ActionDef
    // is invisible to users while tests still pass (invoke() bypasses the
    // catalog) — this pins every handled id to a registration.
    #[test]
    fn every_handled_action_is_registered() {
        let app = test_app();
        let catalog = app.world().resource::<ActionCatalog>();
        for id in [
            "prefab.group",
            "prefab.open",
            "prefab.revert-overrides",
            "prefab.apply-to-prefab",
            "prefab.make-variant",
            "prefab.flatten",
            "prefab.repeat",
            "prefab.bake",
            "prefab.set-kit",
            "prefab.paint",
            "paint.exit",
        ] {
            assert!(
                catalog.get(&ActionId::new(id.to_string())).is_some(),
                "{id} handled but never registered"
            );
        }
    }

    pub(crate) fn invoke(app: &mut App, action: &str) {
        app.world_mut().write_message(ActionInvoked {
            action: ActionId::new(action.to_string()),
            args: None,
            source: InvocationSource::Test,
        });
        app.update();
        app.update();
    }

    fn spawn_loose(app: &mut App, label: &str, payload: f32, at: Vec3) -> SceneId {
        let id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: label.into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id,
                    components: vec![
                        Box::new(Payload(payload)).into_partial_reflect(),
                        Box::new(Transform::from_translation(at)).into_partial_reflect(),
                        Box::new(Name::new(label.to_string())).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        id
    }

    pub(crate) fn cleanup_prefab_file(name: &str) {
        let dir = crate::authoring::prefabs_dir();
        let _ = std::fs::remove_file(dir.join(format!("{name}.prefab.ron")));
        let _ = std::fs::remove_file(dir.join(format!("{name}.prefab.ron.bak")));
        let _ = std::fs::remove_dir(dir);
    }

    // Redesign #1: `g` replaces the selection with an instance IN PLACE, as ONE
    // undoable transaction; undo restores the originals exactly.
    #[test]
    fn group_replaces_selection_undoably() {
        let mut app = test_app();
        let a = spawn_loose(&mut app, "GroupTestA", 1.0, Vec3::new(2.0, 0.0, 0.0));
        let b = spawn_loose(&mut app, "GroupTestB", 2.0, Vec3::new(4.0, 0.0, 0.0));
        for id in [a, b] {
            let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
            app.world_mut().entity_mut(entity).insert(Selected);
        }

        // Commit the prompt (the UI path sets this on Enter).
        app.world_mut().resource_mut::<authoring::GroupCommit>().0 = Some("GroupTestCrate".into());
        app.update();
        app.update();

        // Originals gone, ONE root remains, selected, at the members' centroid.
        let world = app.world_mut();
        assert!(
            world.resource::<SceneIndex>().get(&a).is_none(),
            "a replaced"
        );
        assert!(
            world.resource::<SceneIndex>().get(&b).is_none(),
            "b replaced"
        );
        let roots: Vec<(Entity, SceneId)> = world
            .query_filtered::<(Entity, &SceneId), With<PrefabInstance>>()
            .iter(world)
            .map(|(e, id)| (e, *id))
            .collect();
        assert_eq!(roots.len(), 1, "one instance root");
        let (root_entity, _) = roots[0];
        assert!(
            world.get::<Selected>(root_entity).is_some(),
            "root selected"
        );
        assert_eq!(
            world.get::<Transform>(root_entity).unwrap().translation,
            Vec3::new(3.0, 0.0, 0.0),
            "root at member centroid"
        );
        // Library holds a 2-record template with rebased transforms.
        let library = world.resource::<PrefabLibrary>();
        let def = library
            .prefabs
            .values()
            .find(|p| p.name == "GroupTestCrate")
            .unwrap();
        assert_eq!(def.template.records().count(), 2);

        // ONE undo restores both originals and removes the instance.
        invoke(&mut app, "core.undo");
        let world = app.world_mut();
        assert!(
            world.resource::<SceneIndex>().get(&a).is_some(),
            "undo restores a"
        );
        assert!(
            world.resource::<SceneIndex>().get(&b).is_some(),
            "undo restores b"
        );
        assert_eq!(
            world
                .query_filtered::<Entity, With<PrefabInstance>>()
                .iter(world)
                .count(),
            0,
            "undo removes the instance"
        );
        cleanup_prefab_file("grouptestcrate");
    }

    // Redesign #4: open an instance in place, add an entity, Esc-close — the
    // template grows and EVERY instance updates; bystanders are never adopted.
    #[test]
    fn open_edit_close_propagates() {
        let mut app = test_app();
        let prefab = crate::tests::barrel_prefab();
        let prefab_id = prefab.id;
        prefab
            .save(
                &crate::authoring::prefabs_dir().join("openclosetest.prefab.ron"),
                &app.world().resource::<AppTypeRegistry>().clone().read(),
            )
            .ok();
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab_id, prefab);

        let bystander = spawn_loose(&mut app, "Bystander", 9.0, Vec3::ZERO);
        let mut spawn_instance = |app: &mut App, x: f32| -> SceneId {
            let id = SceneId::random();
            app.world_mut()
                .resource_mut::<EditQueue>()
                .0
                .push(Transaction {
                    label: "Place".into(),
                    gesture: None,
                    ops: vec![Op::Spawn {
                        id,
                        components: vec![
                            Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
                            Box::new(PrefabOverrides::default()).into_partial_reflect(),
                            Box::new(Transform::from_xyz(x, 0.0, 0.0)).into_partial_reflect(),
                        ],
                    }],
                });
            app.update();
            app.update();
            id
        };
        let instance_a = spawn_instance(&mut app, 0.0);
        let instance_b = spawn_instance(&mut app, 10.0);

        // Open instance A (selection → toggle, the `Enter` path).
        {
            let world = app.world_mut();
            let entity = world.resource::<SceneIndex>().get(&instance_a).unwrap();
            world.entity_mut(entity).insert(Selected);
        }
        invoke(&mut app, "prefab.open");
        assert!(
            app.world()
                .resource::<open_mode::OpenInstance>()
                .0
                .is_some(),
            "instance opened in place"
        );
        assert!(app.world().resource::<editor_scene::SceneIoLock>().0);

        // Add an entity while open: it auto-adopts under the root.
        let added = spawn_loose(&mut app, "AddedPart", 42.0, Vec3::new(1.0, 0.0, 0.0));
        app.update();
        {
            let world = app.world_mut();
            let root = world.resource::<SceneIndex>().get(&instance_a).unwrap();
            let added_entity = world.resource::<SceneIndex>().get(&added).unwrap();
            assert_eq!(
                world.get::<ChildOf>(added_entity).map(|c| c.parent()),
                Some(root),
                "insert while open adopts under the root"
            );
            let bystander_entity = world.resource::<SceneIndex>().get(&bystander).unwrap();
            assert!(
                world.get::<ChildOf>(bystander_entity).is_none(),
                "pre-existing loose entities are NEVER adopted"
            );
        }

        // Close (Esc path): template grows, both instances re-stamp with 2 parts.
        invoke(&mut app, "prefab.open");
        app.update();
        let world = app.world_mut();
        assert!(
            world.resource::<open_mode::OpenInstance>().0.is_none(),
            "closed"
        );
        assert!(
            !world.resource::<editor_scene::SceneIoLock>().0,
            "io unlocked"
        );
        let template_len = world
            .resource::<PrefabLibrary>()
            .prefabs
            .get(&prefab_id)
            .unwrap()
            .template
            .records()
            .count();
        assert_eq!(template_len, 2, "template grew by the added part");
        for id in [instance_a, instance_b] {
            let root = world.resource::<SceneIndex>().get(&id).unwrap();
            let children = world.get::<Children>(root).map(|c| c.len()).unwrap_or(0);
            assert_eq!(children, 2, "every instance updated ({id:?})");
        }
        // Scene capture still serializes roots only (+ the bystander).
        let records = capture_scene(world).records().count();
        assert_eq!(records, 3, "two roots + bystander, never expanded trees");
        cleanup_prefab_file("openclosetest");
        crate::tests::cleanup_prefab_file("barrel"); // close() re-saves under the def name
    }
}
// Escape layering (owner grammar): a live selection absorbs one Escape; only
// an empty-handed Escape closes the open instance.
#[cfg(test)]
mod escape_layering {
    use super::tests::*;
    use super::*;
    use editor_core::prelude::*;

    #[test]
    fn escape_clears_selection_while_instance_open() {
        let mut app = test_app();
        let prefab = crate::tests::barrel_prefab();
        let prefab_id = prefab.id;
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab_id, prefab);
        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        {
            let world = app.world_mut();
            let entity = world.resource::<SceneIndex>().get(&root_id).unwrap();
            world.entity_mut(entity).insert(Selected);
        }
        invoke(&mut app, "prefab.open");
        assert!(
            app.world()
                .resource::<open_mode::OpenInstance>()
                .0
                .is_some()
        );

        invoke(&mut app, "core.escape-home");
        let world = app.world_mut();
        let selected = world
            .query_filtered::<(), With<Selected>>()
            .iter(world)
            .count();
        assert_eq!(selected, 0, "escape clears the selection (one layer)");
        assert!(
            world.resource::<open_mode::OpenInstance>().0.is_some(),
            "instance still open after the selection-clearing escape"
        );
        // The SECOND empty-handed escape closes.
        invoke(&mut app, "core.escape-home");
        let world = app.world_mut();
        assert!(
            world.resource::<open_mode::OpenInstance>().0.is_none(),
            "second escape closes the open instance"
        );
        crate::tests::cleanup_prefab_file("barrel");
    }
}

#[cfg(test)]
mod repeat_tests {
    use super::tests::{Payload, invoke, test_app};
    use super::*;
    use crate::authoring::{GroupCommit, GroupPrompt, PromptPurpose};
    use crate::sockets::Socket;

    // Owner ask: socket placement helpers. A socket dragged roughly toward a
    // face lands exactly ON it, aiming out — the two things mating needs and
    // neither of which can be eyeballed.
    #[test]
    fn snap_socket_lands_on_the_pieces_face() {
        let mut app = test_app();
        let socket_id = SceneId::random();
        let root_id = SceneId::random();
        // A 2×2×2 piece at the origin, with a socket floating near its +X side.
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "piece".into(),
                gesture: None,
                ops: vec![
                    Op::Spawn {
                        id: root_id,
                        components: vec![Box::new(Transform::default()).into_partial_reflect()],
                    },
                    Op::Spawn {
                        id: socket_id,
                        components: vec![
                            Box::new(Socket {
                                name: "east".into(),
                                socket_type: "wall".into(),
                            })
                            .into_partial_reflect(),
                            Box::new(Transform::from_xyz(0.8, 0.3, 0.1)).into_partial_reflect(),
                        ],
                    },
                    Op::Reparent {
                        target: socket_id,
                        parent: Some(root_id),
                    },
                ],
            });
        app.update();
        // Give the piece real bounds (an Aabb is what the snap measures).
        {
            let world = app.world_mut();
            let root = world.resource::<SceneIndex>().get(&root_id).unwrap();
            world
                .entity_mut(root)
                .insert(bevy::camera::primitives::Aabb {
                    center: Vec3::ZERO.into(),
                    half_extents: Vec3::ONE.into(),
                });
            let socket = world.resource::<SceneIndex>().get(&socket_id).unwrap();
            world.entity_mut(socket).insert(Selected);
        }
        app.update();

        invoke(&mut app, "socket.snap-face");
        app.update();
        app.update();

        let socket = app
            .world()
            .resource::<SceneIndex>()
            .get(&socket_id)
            .unwrap();
        let placed = *app.world().get::<Transform>(socket).unwrap();
        assert!(
            (placed.translation.x - 1.0).abs() < 1e-4,
            "sits on the +X face: {:?}",
            placed.translation
        );
        assert!(
            (placed.rotation * Vec3::Z).abs_diff_eq(Vec3::X, 1e-4),
            "+Z aims out of the face: {:?}",
            placed.rotation * Vec3::Z
        );
        // Sliding position along the face is preserved — the snap fixes the
        // one axis it must, not the whole placement.
        assert!((placed.translation.y - 0.3).abs() < 1e-4);
    }

    // D10 `o`: each repeat chains a new instance mated at the free end —
    // three walls in a straight run, exactly 2m apart.
    #[test]
    fn repeat_chains_instances() {
        let mut app = test_app();
        let wall_id = Uuid::new_v4();
        let socket = |name: &str, x: f32, dir: Vec3| {
            (
                SceneId::random(),
                None,
                vec![
                    Box::new(Socket {
                        name: name.into(),
                        socket_type: "wall".into(),
                    })
                    .into_partial_reflect(),
                    Box::new(
                        Transform::from_xyz(x, 0.5, 0.0)
                            .with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
                    )
                    .into_partial_reflect(),
                ],
            )
        };
        let def = PrefabDef {
            kit: None,
            id: wall_id,
            name: "Wall".into(),
            template: editor_scene::snapshot_from_parts(vec![
                (
                    SceneId::random(),
                    None,
                    vec![
                        Box::new(Payload(1.0)).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                ),
                socket("west", -1.0, -Vec3::X),
                socket("east", 1.0, Vec3::X),
            ]),
        };
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(wall_id, def);

        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(wall_id)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        {
            let world = app.world_mut();
            let entity = world.resource::<SceneIndex>().get(&root_id).unwrap();
            world.entity_mut(entity).insert(Selected);
        }
        invoke(&mut app, "prefab.repeat");
        app.update();
        invoke(&mut app, "prefab.repeat");
        app.update();

        let world = app.world_mut();
        let mut xs: Vec<f32> = world
            .query_filtered::<&Transform, (With<PrefabInstance>, Without<PrefabStamped>)>()
            .iter(world)
            .map(|t| t.translation.x)
            .collect();
        xs.sort_by(f32::total_cmp);
        assert_eq!(xs.len(), 3, "o o chained two more walls");
        assert!(
            (xs[1] - xs[0] - 2.0).abs() < 1e-3 && (xs[2] - xs[1] - 2.0).abs() < 1e-3,
            "each wall exactly one length further: {xs:?}"
        );
    }

    // Owner ask: "let me pick the sockets to control the chain direction".
    // Selecting a SOCKET chains out of THAT one — the same wall grows west
    // instead of east purely by which socket is selected.
    #[test]
    fn a_selected_socket_steers_the_chain() {
        let mut app = test_app();
        let wall_id = Uuid::new_v4();
        let socket = |name: &str, x: f32, dir: Vec3| {
            (
                SceneId::random(),
                None,
                vec![
                    Box::new(Socket {
                        name: name.into(),
                        socket_type: "wall".into(),
                    })
                    .into_partial_reflect(),
                    Box::new(
                        Transform::from_xyz(x, 0.5, 0.0)
                            .with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
                    )
                    .into_partial_reflect(),
                ],
            )
        };
        let def = PrefabDef {
            kit: None,
            id: wall_id,
            name: "Wall".into(),
            template: editor_scene::snapshot_from_parts(vec![
                (
                    SceneId::random(),
                    None,
                    vec![
                        Box::new(Payload(1.0)).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                ),
                socket("west", -1.0, -Vec3::X),
                socket("east", 1.0, Vec3::X),
            ]),
        };
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(wall_id, def);
        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(wall_id)).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();

        // Select the WEST socket specifically — the heuristic alone would have
        // taken the first free one.
        {
            let world = app.world_mut();
            let west = world
                .query::<(Entity, &Socket)>()
                .iter(world)
                .find(|(_, s)| s.name == "west")
                .map(|(e, _)| e)
                .expect("west socket stamped");
            world.entity_mut(west).insert(Selected);
        }
        invoke(&mut app, "prefab.repeat");
        app.update();
        app.update();

        let world = app.world_mut();
        let mut xs: Vec<f32> = world
            .query_filtered::<&Transform, (With<PrefabInstance>, Without<PrefabStamped>)>()
            .iter(world)
            .map(|t| t.translation.x)
            .collect();
        xs.sort_by(f32::total_cmp);
        assert_eq!(xs.len(), 2, "one wall chained");
        assert!(
            xs[0] < -1.9,
            "picking the WEST socket grew the run westward, not east: {xs:?}"
        );
    }

    // Owner ask: rotate a wall about a socket so the joint stays mated and the
    // far end swings — the move that builds corners and curves.
    #[test]
    fn rotating_pivots_on_the_selected_socket() {
        let mut app = test_app();
        let wall_id = Uuid::new_v4();
        let socket = |name: &str, x: f32, dir: Vec3| {
            (
                SceneId::random(),
                None,
                vec![
                    Box::new(Socket {
                        name: name.into(),
                        socket_type: "wall".into(),
                    })
                    .into_partial_reflect(),
                    Box::new(
                        Transform::from_xyz(x, 0.0, 0.0)
                            .with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
                    )
                    .into_partial_reflect(),
                ],
            )
        };
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(
                wall_id,
                PrefabDef {
                    kit: None,
                    id: wall_id,
                    name: "Wall".into(),
                    template: editor_scene::snapshot_from_parts(vec![
                        (
                            SceneId::random(),
                            None,
                            vec![
                                Box::new(Payload(1.0)).into_partial_reflect(),
                                Box::new(Transform::default()).into_partial_reflect(),
                            ],
                        ),
                        socket("west", -1.0, -Vec3::X),
                        socket("east", 1.0, Vec3::X),
                    ]),
                },
            );
        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(wall_id)).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();

        // Select the WEST socket: that is the joint to keep pinned.
        let (west, pinned_at) = {
            let world = app.world_mut();
            let found = world
                .query::<(Entity, &Socket, &GlobalTransform)>()
                .iter(world)
                .find(|(_, s, _)| s.name == "west")
                .map(|(e, _, g)| (e, g.translation()))
                .expect("west socket stamped");
            world.entity_mut(found.0).insert(Selected);
            found
        };
        app.update();

        // r y 90 ⏎ — a quarter turn about that socket.
        invoke(&mut app, "transform.rotate");
        invoke(&mut app, "transform.axis-y");
        for digit in ["transform.digit-9", "transform.digit-0"] {
            invoke(&mut app, digit);
        }
        invoke(&mut app, "transform.commit");
        app.update();
        app.update();

        let world = app.world_mut();
        let now = world
            .query::<(&Socket, &GlobalTransform)>()
            .iter(world)
            .find(|(s, _)| s.name == "west")
            .map(|(_, g)| g.translation())
            .expect("west socket still there");
        assert!(
            now.abs_diff_eq(pinned_at, 1e-3),
            "the pinned socket did not move: {pinned_at:?} -> {now:?}"
        );
        // ...and the piece actually turned: the far end swung off the X axis.
        let east = world
            .query::<(&Socket, &GlobalTransform)>()
            .iter(world)
            .find(|(s, _)| s.name == "east")
            .map(|(_, g)| g.translation())
            .expect("east socket");
        assert!(
            east.z.abs() > 1.5,
            "the far end swung round the pivot: {east:?}"
        );
        let _ = west;
    }

    // Owner ask: mate WHILE dragging, so a wall rotated 90° snaps into a corner
    // in front of you instead of after you let go.
    #[test]
    fn dragging_snaps_sockets_live() {
        let mut app = test_app();
        let wall_id = Uuid::new_v4();
        let socket = |name: &str, x: f32, dir: Vec3| {
            (
                SceneId::random(),
                None,
                vec![
                    Box::new(Socket {
                        name: name.into(),
                        socket_type: "wall".into(),
                    })
                    .into_partial_reflect(),
                    Box::new(
                        Transform::from_xyz(x, 0.0, 0.0)
                            .with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
                    )
                    .into_partial_reflect(),
                ],
            )
        };
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(
                wall_id,
                PrefabDef {
                    kit: None,
                    id: wall_id,
                    name: "Wall".into(),
                    template: editor_scene::snapshot_from_parts(vec![
                        (
                            SceneId::random(),
                            None,
                            vec![
                                Box::new(Payload(1.0)).into_partial_reflect(),
                                Box::new(Transform::default()).into_partial_reflect(),
                            ],
                        ),
                        socket("west", -1.0, -Vec3::X),
                        socket("east", 1.0, Vec3::X),
                    ]),
                },
            );
        let place = |app: &mut App, at: Transform| {
            let id = SceneId::random();
            app.world_mut()
                .resource_mut::<EditQueue>()
                .0
                .push(Transaction {
                    label: "place".into(),
                    gesture: None,
                    ops: vec![Op::Spawn {
                        id,
                        components: vec![
                            Box::new(PrefabInstance(wall_id)).into_partial_reflect(),
                            Box::new(at).into_partial_reflect(),
                        ],
                    }],
                });
            app.update();
            app.update();
            id
        };
        // An anchor wall, and a second one turned 90° — the corner case.
        let _anchor = place(&mut app, Transform::default());
        let turned = place(
            &mut app,
            Transform::from_xyz(4.0, 0.0, 0.0)
                .with_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2)),
        );

        // Drag it toward the anchor's east socket and STOP short — no commit.
        {
            let world = app.world_mut();
            let entity = world.resource::<SceneIndex>().get(&turned).unwrap();
            world.entity_mut(entity).insert(Selected);
        }
        invoke(&mut app, "transform.move");
        app.world_mut().resource_mut::<GestureMotion>().world = Some(Vec3::new(-2.2, 0.0, 0.0));
        app.update();
        app.update();

        // Mid-drag, with the gesture still ACTIVE, it must already be mated.
        assert!(
            matches!(
                *app.world().resource::<MoveGesture>(),
                MoveGesture::Active { .. }
            ),
            "still dragging"
        );
        let entity = app.world().resource::<SceneIndex>().get(&turned).unwrap();
        let at = app.world().get::<Transform>(entity).unwrap().translation;
        assert!(
            (at.x - 2.0).abs() < 1e-3,
            "snapped onto the anchor's east socket mid-drag: {at:?}"
        );
    }

    // Owner rule: a prefab selects as a UNIT until opened. Its members carry
    // SceneIds and would otherwise be clicked directly, authoring an override
    // on a piece you never meant to step into.
    #[test]
    fn instances_seal_until_opened() {
        let mut app = test_app();
        let prefab = crate::tests::barrel_prefab();
        let prefab_id = prefab.id;
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab_id, prefab);
        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();

        let root = app.world().resource::<SceneIndex>().get(&root_id).unwrap();
        assert!(
            app.world().get::<SelectionSealed>(root).is_some(),
            "a closed instance is sealed"
        );

        // Opening lifts the seal for THAT instance — that is what edit mode is.
        app.world_mut().entity_mut(root).insert(Selected);
        invoke(&mut app, "prefab.open");
        app.update();
        app.update();
        assert!(
            app.world().get::<SelectionSealed>(root).is_none(),
            "the OPEN instance is selectable inside"
        );

        // Closing seals it again.
        invoke(&mut app, "prefab.open");
        app.update();
        app.update();
        let root = app.world().resource::<SceneIndex>().get(&root_id).unwrap();
        assert!(
            app.world().get::<SelectionSealed>(root).is_some(),
            "closing re-seals it"
        );
        crate::tests::cleanup_prefab_file("barrel");
    }

    // Owner ask: lay a whole run at once instead of pressing `o` thirty times.
    // Five pieces, evenly spaced, in ONE undo entry.
    #[test]
    fn fill_lays_an_even_run_in_one_entry() {
        let mut app = test_app();
        let wall_id = Uuid::new_v4();
        let socket = |name: &str, x: f32, dir: Vec3| {
            (
                SceneId::random(),
                None,
                vec![
                    Box::new(Socket {
                        name: name.into(),
                        socket_type: "wall".into(),
                    })
                    .into_partial_reflect(),
                    Box::new(
                        Transform::from_xyz(x, 0.5, 0.0)
                            .with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
                    )
                    .into_partial_reflect(),
                ],
            )
        };
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(
                wall_id,
                PrefabDef {
                    kit: None,
                    id: wall_id,
                    name: "Wall".into(),
                    template: editor_scene::snapshot_from_parts(vec![
                        (
                            SceneId::random(),
                            None,
                            vec![
                                Box::new(Payload(1.0)).into_partial_reflect(),
                                Box::new(Transform::default()).into_partial_reflect(),
                            ],
                        ),
                        socket("west", -1.0, -Vec3::X),
                        socket("east", 1.0, Vec3::X),
                    ]),
                },
            );
        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(wall_id)).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        {
            let world = app.world_mut();
            let entity = world.resource::<SceneIndex>().get(&root_id).unwrap();
            world.entity_mut(entity).insert(Selected);
        }
        let depth_before = app.world().resource::<History>().undo_depth();

        // The prompt's commit path, exactly as Enter drives it.
        app.world_mut().resource_mut::<GroupPrompt>().purpose = PromptPurpose::Fill;
        app.world_mut().resource_mut::<GroupCommit>().0 = Some("5".into());
        app.update();
        app.update();

        let world = app.world_mut();
        let mut xs: Vec<f32> = world
            .query_filtered::<&Transform, (With<PrefabInstance>, Without<PrefabStamped>)>()
            .iter(world)
            .map(|t| t.translation.x)
            .collect();
        xs.sort_by(f32::total_cmp);
        assert_eq!(xs.len(), 6, "the original plus five filled: {xs:?}");
        for pair in xs.windows(2) {
            assert!(
                (pair[1] - pair[0] - 2.0).abs() < 1e-3,
                "evenly spaced one length apart: {xs:?}"
            );
        }
        // The chained first piece and the filled remainder are ONE gesture as
        // far as the user is concerned — at most two entries, never five.
        let entries = app.world().resource::<History>().undo_depth() - depth_before;
        assert!(
            entries <= 2,
            "fill is not five separate undo entries: {entries}"
        );
    }

    // The step must come from the piece the fill actually CHAINED off, not from
    // whichever instance an unordered query returned last. The original fixture
    // held exactly one instance, so the bug could not show; a real scene holds
    // many, and the run scattered. Three unrelated instances, deliberately at
    // awkward poses, make the difference observable.
    #[test]
    fn fill_measures_its_step_from_the_chained_piece() {
        let mut app = test_app();
        let wall_id = Uuid::new_v4();
        let socket = |name: &str, x: f32, dir: Vec3| {
            (
                SceneId::random(),
                None,
                vec![
                    Box::new(Socket {
                        name: name.into(),
                        socket_type: "wall".into(),
                    })
                    .into_partial_reflect(),
                    Box::new(
                        Transform::from_xyz(x, 0.5, 0.0)
                            .with_rotation(Quat::from_rotation_arc(Vec3::Z, dir)),
                    )
                    .into_partial_reflect(),
                ],
            )
        };
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(
                wall_id,
                PrefabDef {
                    kit: None,
                    id: wall_id,
                    name: "Wall".into(),
                    template: editor_scene::snapshot_from_parts(vec![
                        (
                            SceneId::random(),
                            None,
                            vec![
                                Box::new(Payload(1.0)).into_partial_reflect(),
                                Box::new(Transform::default()).into_partial_reflect(),
                            ],
                        ),
                        socket("west", -1.0, -Vec3::X),
                        socket("east", 1.0, Vec3::X),
                    ]),
                },
            );
        // Three unrelated instances of the same prefab, scattered and turned —
        // exactly what a half-built level looks like.
        let decoys: Vec<SceneId> = (0..3).map(|_| SceneId::random()).collect();
        let scattered = [
            Transform::from_xyz(37.0, 5.0, -12.0).with_rotation(Quat::from_rotation_y(0.7)),
            Transform::from_xyz(-8.0, 0.0, 21.0).with_rotation(Quat::from_rotation_y(2.1)),
            Transform::from_xyz(3.0, -4.0, 9.0),
        ];
        // The run's piece is spawned FIRST, so the scattered ones come after it
        // in every query order. That is the arrangement the old code got wrong:
        // it took the last id the query happened to return, which is a decoy.
        let root_id = SceneId::random();
        let mut ops: Vec<Op> = vec![Op::Spawn {
            id: root_id,
            components: vec![
                Box::new(PrefabInstance(wall_id)).into_partial_reflect(),
                Box::new(Transform::default()).into_partial_reflect(),
            ],
        }];
        ops.extend(decoys.iter().zip(scattered).map(|(id, at)| Op::Spawn {
            id: *id,
            components: vec![
                Box::new(PrefabInstance(wall_id)).into_partial_reflect(),
                Box::new(at).into_partial_reflect(),
            ],
        }));
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "place".into(),
                gesture: None,
                ops,
            });
        app.update();
        app.update();
        // Chain from the piece's SOCKET, which is how a direction gets chosen.
        // The root then stays in the same archetype as the scattered instances,
        // so it is emphatically not the last id a query returns — which is the
        // whole point: the source has to be known, not guessed at.
        {
            let world = app.world_mut();
            let root = world.resource::<SceneIndex>().get(&root_id).unwrap();
            let socket = world
                .query::<(Entity, &Socket, &ChildOf)>()
                .iter(world)
                .find(|(_, socket, parent)| parent.parent() == root && socket.name == "east")
                .map(|(entity, _, _)| entity)
                .expect("the stamped instance carries its east socket");
            world.entity_mut(socket).insert(Selected);
        }

        app.world_mut().resource_mut::<GroupPrompt>().purpose = PromptPurpose::Fill;
        app.world_mut().resource_mut::<GroupCommit>().0 = Some("4".into());
        app.update();
        app.update();

        let world = app.world_mut();
        let mut run: Vec<Vec3> = world
            .query_filtered::<(&SceneId, &Transform), (With<PrefabInstance>, Without<PrefabStamped>)>()
            .iter(world)
            .filter(|(id, _)| !decoys.contains(id))
            .map(|(_, t)| t.translation)
            .collect();
        run.sort_by(|a, b| a.x.total_cmp(&b.x));
        assert_eq!(run.len(), 5, "the original plus four filled: {run:?}");
        for pair in run.windows(2) {
            let step = pair[1] - pair[0];
            assert!(
                (step.x - 2.0).abs() < 1e-3 && step.y.abs() < 1e-3 && step.z.abs() < 1e-3,
                "one wall length apart along x, and nowhere else: {run:?}"
            );
        }
    }
    // Owner report: "sockets don't move when the transform is edited". Editing a
    // socket IS editing an ordinary member — the same `Set` the inspector emits
    // must land on the entity and move it.
    #[test]
    fn editing_a_socket_transform_moves_it() {
        let mut app = test_app();
        let socket_id = SceneId::random();
        let def = PrefabDef {
            kit: None,
            id: Uuid::new_v4(),
            name: "Socketed".into(),
            template: editor_scene::snapshot_from_parts(vec![
                (
                    SceneId::random(),
                    None,
                    vec![
                        Box::new(Payload(1.0)).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                ),
                (
                    socket_id,
                    None,
                    vec![
                        Box::new(Socket {
                            name: "east".into(),
                            socket_type: "wall".into(),
                        })
                        .into_partial_reflect(),
                        Box::new(Transform::from_xyz(1.0, 0.0, 0.0)).into_partial_reflect(),
                    ],
                ),
            ]),
        };
        let prefab = def.id;
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab, def);
        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(prefab)).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();

        // The stamped socket, addressed by its own SceneId — what the inspector
        // edits when you select the socket row.
        let (socket_entity, socket_scene_id) = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<(Entity, &SceneId), With<Socket>>();
            let found: Vec<(Entity, SceneId)> = query.iter(world).map(|(e, id)| (e, *id)).collect();
            assert_eq!(found.len(), 1, "the instance stamped its socket");
            found[0]
        };
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Move Socket".into(),
                gesture: None,
                ops: vec![Op::Set {
                    target: socket_scene_id,
                    value: Box::new(Transform::from_xyz(1.0, 3.0, 0.0)).into_partial_reflect(),
                }],
            });
        app.update();
        app.update();

        let moved = app
            .world()
            .get::<Transform>(socket_entity)
            .map(|t| t.translation);
        assert_eq!(
            moved,
            Some(Vec3::new(1.0, 3.0, 0.0)),
            "editing a socket's Transform moves the socket"
        );

        // ...and the AUTHORING path: inside an open instance, where the edit is
        // meant to reach the template rather than be re-stamped away.
        {
            let world = app.world_mut();
            let root = world.resource::<SceneIndex>().get(&root_id).unwrap();
            world.entity_mut(root).insert(Selected);
        }
        invoke(&mut app, "prefab.open");
        app.update();
        app.update();
        let (open_socket, open_socket_id) = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<(Entity, &SceneId), With<Socket>>();
            let found: Vec<(Entity, SceneId)> = query.iter(world).map(|(e, id)| (e, *id)).collect();
            assert_eq!(found.len(), 1, "one socket while open");
            found[0]
        };
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Move Socket".into(),
                gesture: None,
                ops: vec![Op::Set {
                    target: open_socket_id,
                    value: Box::new(Transform::from_xyz(1.0, 7.0, 0.0)).into_partial_reflect(),
                }],
            });
        app.update();
        app.update();
        assert_eq!(
            app.world()
                .get::<Transform>(open_socket)
                .map(|t| t.translation),
            Some(Vec3::new(1.0, 7.0, 0.0)),
            "editing a socket inside an OPEN instance moves it"
        );
        // Opening can persist the prefab — never leave one behind.
        crate::tests::cleanup_prefab_file("socketed");
    }
}

#[cfg(test)]
mod template_tests {
    use super::*;
    use crate::template_mode::{TemplateEdit, TemplateRequests};
    use crate::tests::{barrel_prefab, test_app};

    /// The edit must reach DISK. Bumping the generation updates the instances on
    /// screen and says "every instance follows" — and then the next launch loads
    /// the old file and the work is gone. Every other prefab-editing path saves;
    /// this one did not, and nothing noticed because everything on screen looked
    /// right.
    #[test]
    fn editing_the_template_writes_the_prefab_to_disk() {
        let mut app = test_app();
        let mut prefab = barrel_prefab();
        // A name of our own, so the test owns its file.
        prefab.name = format!("probe-template-{}", uuid::Uuid::new_v4().simple());
        let prefab_id = prefab.id;
        let path = crate::authoring::prefabs_dir().join(format!(
            "{}.prefab.ron",
            prefab.name.to_lowercase().replace(' ', "-")
        ));
        let before = prefab.template.records().count();
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab_id, prefab);
        let instance = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: instance,
                    components: vec![
                        Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&instance).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);

        invoke(&mut app, "prefab.edit-template");
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "add a part".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: SceneId::random(),
                    components: vec![
                        Box::new(Transform::from_xyz(0.0, 2.0, 0.0)).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        invoke(&mut app, "prefab.close-template");

        let written = std::fs::read_to_string(&path);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("ron.bak"));
        let written = written.expect("the prefab file exists after editing it");
        // The added part is IN the file, not just in memory.
        let records = written.matches("id:").count();
        assert!(
            records > before,
            "the edit reached disk ({records} records for {before} before)"
        );
    }
    fn invoke(app: &mut App, action: &'static str) {
        app.world_mut().write_message(ActionInvoked {
            action: ActionId::new_static(action),
            args: None,
            source: InvocationSource::Test,
        });
        app.update();
        app.update();
    }

    /// The level is PARKED, not rebuilt: editing a prefab and coming back must
    /// leave the level exactly as it was, or a world swap is a way to lose work
    /// — which is what killed v1's prefab UX.
    #[test]
    fn the_level_survives_a_trip_into_the_prefab() {
        let mut app = test_app();
        let prefab = barrel_prefab();
        let prefab_id = prefab.id;
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab_id, prefab);

        // A level: one instance and one plain entity beside it.
        let instance = SceneId::random();
        let bystander = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "level".into(),
                gesture: None,
                ops: vec![
                    Op::Spawn {
                        id: instance,
                        components: vec![
                            Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
                            Box::new(Transform::from_xyz(5.0, 0.0, 0.0)).into_partial_reflect(),
                        ],
                    },
                    Op::Spawn {
                        id: bystander,
                        components: vec![
                            Box::new(Transform::from_xyz(-3.0, 1.0, 0.0)).into_partial_reflect(),
                        ],
                    },
                ],
            });
        app.update();
        app.update();

        let entity = app.world().resource::<SceneIndex>().get(&instance).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);

        invoke(&mut app, "prefab.edit-template");
        assert!(
            app.world().resource::<TemplateEdit>().active(),
            "the prefab opened"
        );
        // The LEVEL is not in the world any more — the bystander is parked with it.
        assert!(
            app.world()
                .resource::<SceneIndex>()
                .get(&bystander)
                .is_none(),
            "the level stepped aside"
        );

        invoke(&mut app, "prefab.close-template");
        assert!(!app.world().resource::<TemplateEdit>().active());
        assert!(
            app.world()
                .resource::<SceneIndex>()
                .get(&bystander)
                .is_some(),
            "and came back whole"
        );
        let restored = app.world().resource::<SceneIndex>().get(&instance).unwrap();
        assert_eq!(
            app.world().get::<Transform>(restored).unwrap().translation,
            Vec3::new(5.0, 0.0, 0.0),
            "with the instance where it was"
        );
    }

    /// Editing the TEMPLATE changes the prefab — that is the whole difference
    /// from editing an instance, which would only have made an override.
    #[test]
    fn editing_the_template_changes_the_prefab() {
        let mut app = test_app();
        let prefab = barrel_prefab();
        let prefab_id = prefab.id;
        let before = prefab.template.records().count();
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab_id, prefab);
        let instance = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: instance,
                    components: vec![
                        Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&instance).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);

        invoke(&mut app, "prefab.edit-template");
        // Add a part to the PREFAB while its own scene is open.
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "add a part".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: SceneId::random(),
                    components: vec![
                        Box::new(Transform::from_xyz(0.0, 2.0, 0.0)).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        invoke(&mut app, "prefab.close-template");

        let after = app
            .world()
            .resource::<PrefabLibrary>()
            .prefabs
            .get(&prefab_id)
            .unwrap()
            .template
            .records()
            .count();
        assert_eq!(after, before + 1, "the prefab itself grew a part");
    }

    /// Saving while the level is parked would write the PREFAB over it. The
    /// refusal is the feature.
    #[test]
    fn scene_io_is_refused_while_the_prefab_is_open() {
        let mut app = test_app();
        let prefab = barrel_prefab();
        let prefab_id = prefab.id;
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab_id, prefab);
        let instance = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: instance,
                    components: vec![
                        Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
                        Box::new(Transform::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();
        let entity = app.world().resource::<SceneIndex>().get(&instance).unwrap();
        app.world_mut().entity_mut(entity).insert(Selected);
        invoke(&mut app, "prefab.edit-template");

        app.world_mut().write_message(ActionInvoked {
            action: ActionId::new_static("scene.save"),
            args: None,
            source: InvocationSource::Test,
        });
        app.update();
        let refused = app
            .world_mut()
            .resource_mut::<bevy::ecs::message::Messages<editor_scene::SceneIoFeedback>>()
            .drain()
            .any(|message| !message.success && message.message.contains("back to the level"));
        assert!(refused, "saving was refused, out loud");
    }

    /// Nothing selected is not a prefab: say so rather than opening something.
    #[test]
    fn it_needs_to_know_which_prefab() {
        let mut app = test_app();
        invoke(&mut app, "prefab.edit-template");
        assert!(!app.world().resource::<TemplateEdit>().active());
    }

    fn _unused(_: &TemplateRequests) {}
}
