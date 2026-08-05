//! Material assets + library (M3-C6, spec §5/§7): a material is a VERSIONED asset
//! — `materials.ron` carries the same envelope discipline as scenes (format
//! version, atomic temp+rename save with a `.bak`, forward-compat via serde
//! defaults). Scenes reference materials by asset id only (`MaterialRef`), so a
//! scene file never embeds material data and survives library edits.
//!
//! Assignment is an ordinary `Set` transaction (one undo entry). Library PARAM
//! edits save immediately but are not yet undoable — the edit history is scoped
//! to scene state; asset-history is an M4 concern (noted at the gate).

use bevy::prelude::*;
use editor_core::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use uuid::Uuid;

pub const MATERIALS_FORMAT_VERSION: u32 = 1;

/// Scene-side reference: which library material shades this entity. Serialized
/// with the scene BY ID — never by value.
#[derive(Component, Reflect, Clone, Copy, PartialEq, Debug, Default)]
#[reflect(Component)]
pub struct MaterialRef(pub Uuid);

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
#[serde(default)]
pub struct MaterialDef {
    pub id: Uuid,
    pub name: String,
    pub base_color: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
}

impl Default for MaterialDef {
    fn default() -> Self {
        Self {
            id: Uuid::nil(),
            name: "Material".into(),
            base_color: [0.8, 0.8, 0.8, 1.0],
            metallic: 0.0,
            roughness: 0.6,
        }
    }
}

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct MaterialsEnvelope {
    format_version: u32,
    materials: Vec<MaterialDef>,
}

#[derive(Resource)]
pub struct MaterialLibrary {
    pub materials: Vec<MaterialDef>,
    pub path: PathBuf,
    /// Bumped on every library mutation — visual sync and saves key off this.
    pub generation: u64,
}

impl Default for MaterialLibrary {
    fn default() -> Self {
        Self {
            materials: Vec::new(),
            path: PathBuf::from("materials.ron"),
            generation: 0,
        }
    }
}

impl MaterialLibrary {
    pub fn get(&self, id: &Uuid) -> Option<&MaterialDef> {
        self.materials.iter().find(|m| &m.id == id)
    }
    pub fn get_mut(&mut self, id: &Uuid) -> Option<&mut MaterialDef> {
        self.generation += 1;
        self.materials.iter_mut().find(|m| &m.id == id)
    }
    pub fn add(&mut self, def: MaterialDef) {
        self.generation += 1;
        self.materials.push(def);
    }
}

#[derive(Debug)]
pub enum MaterialsError {
    Io(std::io::Error),
    Format(String),
    FutureVersion { found: u32, supported: u32 },
}

pub fn save_materials(library: &MaterialLibrary, path: &Path) -> Result<(), MaterialsError> {
    let envelope = MaterialsEnvelope {
        format_version: MATERIALS_FORMAT_VERSION,
        materials: library.materials.clone(),
    };
    let text = ron::ser::to_string_pretty(&envelope, ron::ser::PrettyConfig::default())
        .map_err(|e| MaterialsError::Format(e.to_string()))?;
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, &text).map_err(MaterialsError::Io)?;
    if path.exists() {
        let bak = path.with_extension("ron.bak");
        let _ = std::fs::copy(path, bak);
    }
    std::fs::rename(&tmp, path).map_err(MaterialsError::Io)?;
    Ok(())
}

/// Non-destructive load: parse fully before touching the resource; unknown FUTURE
/// versions refuse loudly (same contract as scenes).
pub fn load_materials(path: &Path) -> Result<Vec<MaterialDef>, MaterialsError> {
    let text = std::fs::read_to_string(path).map_err(MaterialsError::Io)?;
    let envelope: MaterialsEnvelope =
        ron::from_str(&text).map_err(|e| MaterialsError::Format(e.to_string()))?;
    if envelope.format_version > MATERIALS_FORMAT_VERSION {
        return Err(MaterialsError::FutureVersion {
            found: envelope.format_version,
            supported: MATERIALS_FORMAT_VERSION,
        });
    }
    Ok(envelope.materials)
}

pub(crate) fn load_library_at_startup(mut library: ResMut<MaterialLibrary>) {
    let path = library.path.clone();
    match load_materials(&path) {
        Ok(materials) => {
            library.materials = materials;
            library.generation += 1;
        }
        Err(MaterialsError::Io(_)) => {} // no library yet — starts empty
        Err(e) => error!("materials library load failed: {e:?} — starting empty"),
    }
}

/// Library mutations persist immediately (atomic).
pub(crate) fn save_library_on_change(library: Res<MaterialLibrary>, mut last_saved: Local<u64>) {
    if library.generation == *last_saved || library.generation == 0 {
        return;
    }
    *last_saved = library.generation;
    let path = library.path.clone();
    if let Err(e) = save_materials(&library, &path) {
        error!("materials library save failed: {e:?}");
    }
}

/// `material.new`: append a fresh material and report it (the palette lists it
/// immediately; params are tuned from the inspector once assigned).
pub(crate) fn handle_material_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut library: ResMut<MaterialLibrary>,
    mut feedback: MessageWriter<crate::SceneIoFeedback>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() == "material.new" {
            let count = library.materials.len() + 1;
            let def = MaterialDef {
                id: Uuid::new_v4(),
                name: format!("Material {count}"),
                ..Default::default()
            };
            let name = def.name.clone();
            library.add(def);
            feedback.write(crate::SceneIoFeedback {
                message: format!("created {name}"),
                success: true,
            });
        }
    }
}

pub(crate) struct MaterialsFeature;

impl EditorFeature for MaterialsFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("materials", "Material Library")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.component::<MaterialRef>()
            .action(
                ActionDef::new("material.new", "New Material")
                    .describe("Create a material in the library")
                    .context("normal"),
            )
            .action(
                ActionDef::new("material.assign", "Assign Material")
                    .describe("Pick a library material for the selection")
                    .context("normal")
                    .bind("space m"),
            );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::prelude::{History, HistoryRequests};

    // C6: assignment is ONE undoable transaction; undo removes the reference.
    #[test]
    fn assignment_is_undoable() {
        let mut app = App::new();
        app.add_plugins(editor_core::EditorCorePlugin);
        struct TestFeature;
        impl EditorFeature for TestFeature {
            fn manifest(&self) -> FeatureManifest {
                FeatureManifest::new("mat-test", "Mat Test")
            }
            fn register(&self, reg: &mut FeatureRegistry) {
                reg.component::<MaterialRef>();
            }
        }
        app.add_editor_feature(TestFeature);
        app.init_resource::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>();
        app.finish();
        app.update();

        let (a, b) = (SceneId::random(), SceneId::random());
        for id in [a, b] {
            app.world_mut()
                .resource_mut::<EditQueue>()
                .0
                .push(Transaction {
                    label: "spawn".into(),
                    gesture: None,
                    ops: vec![Op::Spawn {
                        id,
                        components: vec![],
                    }],
                });
        }
        app.update();

        let material = Uuid::new_v4();
        let depth = app.world().resource::<History>().undo_depth();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Assign Material".into(),
                gesture: None,
                ops: [a, b]
                    .into_iter()
                    .map(|target| Op::Set {
                        target,
                        value: Box::new(MaterialRef(material)).into_partial_reflect(),
                    })
                    .collect(),
            });
        app.update();
        let world = app.world_mut();
        let assigned = world
            .query::<&MaterialRef>()
            .iter(world)
            .filter(|m| m.0 == material)
            .count();
        assert_eq!(assigned, 2);
        assert_eq!(
            world.resource::<History>().undo_depth(),
            depth + 1,
            "one entry"
        );

        world.resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        let world = app.world_mut();
        assert_eq!(
            world.query::<&MaterialRef>().iter(world).count(),
            0,
            "undo removes"
        );
    }

    // C6: versioned envelope round-trips byte-identically; future versions refuse.
    #[test]
    fn library_round_trip_and_versioning() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("materials.ron");
        let mut library = MaterialLibrary::default();
        library.add(MaterialDef {
            id: Uuid::new_v4(),
            name: "Rust".into(),
            base_color: [0.7, 0.3, 0.1, 1.0],
            metallic: 0.9,
            roughness: 0.3,
        });
        save_materials(&library, &path).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();

        let loaded = load_materials(&path).unwrap();
        assert_eq!(loaded, library.materials);
        let mut reloaded = MaterialLibrary::default();
        reloaded.materials = loaded;
        save_materials(&reloaded, &path).unwrap();
        let second = std::fs::read_to_string(&path).unwrap();
        assert_eq!(first, second, "save -> load -> save byte-identical");

        std::fs::write(&path, "(format_version: 99, materials: [])").unwrap();
        assert!(matches!(
            load_materials(&path),
            Err(MaterialsError::FutureVersion { found: 99, .. })
        ));
    }
}
