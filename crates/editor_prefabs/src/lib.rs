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

pub mod overrides;
pub use overrides::{sync_overrides, StampedFrom};

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
    pub template: SceneSnapshot,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct PrefabHeader {
    format_version: u32,
    id: Uuid,
    name: String,
    /// The template, in THE scene envelope format (nested document).
    template: String,
}

impl Default for PrefabHeader {
    fn default() -> Self {
        Self {
            format_version: PREFAB_FORMAT_VERSION,
            id: Uuid::nil(),
            name: String::new(),
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
    pub fn save(&self, path: &Path, registry: &bevy::reflect::TypeRegistry) -> Result<(), PrefabError> {
        let header = PrefabHeader {
            format_version: PREFAB_FORMAT_VERSION,
            id: self.id,
            name: self.name.clone(),
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
        Ok(Self { id: header.id, name: header.name, template })
    }
}

/// Loaded prefabs by id.
#[derive(Resource, Default)]
pub struct PrefabLibrary {
    pub prefabs: HashMap<Uuid, PrefabDef>,
}

/// Stamp a prefab's template under an instance root: fresh runtime SceneIds
/// (mapped from template-local ids), `PrefabStamped` markers (capture-excluded),
/// components applied through the same reflection path as scene load.
pub fn stamp_prefab(world: &mut World, prefab_id: Uuid, root: Entity) {
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();

    // Collect the template (clone values out so the library borrow ends).
    let records: Vec<(SceneId, Option<SceneId>, Vec<Box<dyn bevy::reflect::PartialReflect>>)> = {
        let library = world.resource::<PrefabLibrary>();
        let Some(prefab) = library.prefabs.get(&prefab_id) else { return };
        prefab
            .template
            .records()
            .map(|(id, parent, components)| {
                (id, parent, components.iter().map(|c| c.to_dynamic()).collect())
            })
            .collect()
    };

    let root_scene_id = world.get::<SceneId>(root).copied().unwrap_or_default();
    let patches: Vec<OverridePatch> =
        world.get::<PrefabOverrides>(root).map(|o| o.0.clone()).unwrap_or_default();

    let mut spawned: HashMap<SceneId, Entity> = HashMap::new();
    for (template_id, _, components) in &records {
        let entity = world
            .spawn((
                SceneId::random(),
                PrefabStamped,
                StampedFrom { instance_root: root_scene_id, template_id: *template_id },
            ))
            .id();
        for value in components {
            let Some(info) = value.get_represented_type_info() else { continue };
            let Some(registration) = registry.get(info.type_id()) else { continue };
            let Some(reflect_component) =
                registration.data::<bevy::ecs::reflect::ReflectComponent>()
            else {
                continue;
            };
            let Ok(mut entity_mut) = world.get_entity_mut(entity) else { continue };
            reflect_component.apply_or_insert_mapped(
                &mut entity_mut,
                value.as_ref(),
                &registry,
                &mut (),
                RelationshipHookMode::Run,
            );
        }
        // Overrides re-apply OVER the template (per-field patches).
        for patch in patches.iter().filter(|p| p.entity == template_id.0.to_string()) {
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
            {
                if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
                    reflect_component.apply_or_insert_mapped(
                        &mut entity_mut,
                        dynamic.as_ref(),
                        &registry,
                        &mut (),
                        RelationshipHookMode::Run,
                    );
                }
            }
        }
        spawned.insert(*template_id, entity);
    }
    // Parent wiring: template-internal parents, roots under the instance root.
    for (template_id, parent, _) in &records {
        let Some(&child) = spawned.get(template_id) else { continue };
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
        app.init_resource::<PrefabLibrary>();
        app.init_resource::<overrides::OverrideCursor>();
        app.add_editor_feature(PrefabsFeature);
        app.add_systems(
            Update,
            (stamp_new_instances, overrides::sync_overrides)
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
        reg.component::<PrefabInstance>().component::<PrefabOverrides>();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_scene::{capture_scene, snapshot_from_parts};

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component)]
    struct Payload(f32);

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("prefab-test", "Prefab Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Payload>().component::<Transform>().component::<Name>();
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(editor_core::EditorCorePlugin);
        app.add_plugins(EditorPrefabsPlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>();
        app.finish();
        app.update();
        app
    }

    fn barrel_prefab() -> PrefabDef {
        let child = SceneId::random();
        PrefabDef {
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
        let prefab = barrel_prefab();
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
        app.world_mut().resource_mut::<PrefabLibrary>().prefabs.insert(prefab_id, prefab);

        // Spawn an instance through the EDIT path (undoable like everything).
        let root_id = SceneId::random();
        app.world_mut().resource_mut::<EditQueue>().0.push(Transaction {
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
}
