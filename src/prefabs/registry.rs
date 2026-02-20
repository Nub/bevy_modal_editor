use bevy::prelude::*;
use bevy_editor_game::MaterialLibrary;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use bevy_vfx::VfxSystem;

/// Cached metadata about a discovered prefab directory.
#[derive(Clone, Debug)]
pub struct PrefabEntry {
    /// Display name (directory name, e.g. "fireball")
    pub name: String,
    /// Full path to the prefab directory
    pub directory: PathBuf,
    /// Full path to prefab.scene inside the directory
    pub scene_path: PathBuf,
    /// Material library loaded from prefab.meta
    pub material_library: MaterialLibrary,
    /// Particle library loaded from prefab.meta
    pub particle_presets: HashMap<String, VfxSystem>,
}

/// Sidecar metadata stored alongside prefab.scene (same format as scene metadata).
#[derive(Serialize, Deserialize, Default)]
struct PrefabMetadata {
    #[serde(default)]
    material_library: MaterialLibrary,
    #[serde(default)]
    particle_presets: HashMap<String, VfxSystem>,
}

/// Resource containing all discovered prefab directories from assets/prefabs/.
#[derive(Resource)]
pub struct PrefabRegistry {
    pub entries: HashMap<String, PrefabEntry>,
    pub root_directory: PathBuf,
    next_instance_counters: HashMap<String, u32>,
}

impl Default for PrefabRegistry {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            root_directory: PathBuf::from("assets/prefabs"),
            next_instance_counters: HashMap::new(),
        }
    }
}

impl PrefabRegistry {
    /// Generate a unique instance ID for a prefab (e.g. "fireball_1", "fireball_2")
    pub fn next_instance_id(&mut self, prefab_name: &str) -> String {
        let counter = self.next_instance_counters.entry(prefab_name.to_string()).or_insert(0);
        *counter += 1;
        format!("{}_{}", prefab_name, counter)
    }

    /// Get a prefab entry by name
    pub fn get(&self, name: &str) -> Option<&PrefabEntry> {
        self.entries.get(name)
    }

    /// List all prefab names
    pub fn names(&self) -> Vec<&str> {
        self.entries.keys().map(|s| s.as_str()).collect()
    }

    /// Scan the prefab root directory for subdirectories containing prefab.scene
    pub fn refresh(&mut self) {
        self.entries.clear();

        if !self.root_directory.exists() {
            if let Err(e) = fs::create_dir_all(&self.root_directory) {
                error!("Failed to create prefabs directory: {}", e);
                return;
            }
        }

        let entries = match fs::read_dir(&self.root_directory) {
            Ok(e) => e,
            Err(e) => {
                error!("Failed to read prefabs directory: {}", e);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }

            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Look for {name}.scn.ron inside the directory
            let scene_path = path.join(format!("{}.scn.ron", name));
            if !scene_path.exists() {
                continue;
            }

            // Load metadata sidecar if present ({name}.scn.ron.meta)
            let meta_path = path.join(format!("{}.scn.ron.meta", name));
            let metadata = if meta_path.exists() {
                fs::read_to_string(&meta_path)
                    .ok()
                    .and_then(|content| ron::from_str::<PrefabMetadata>(&content).ok())
                    .unwrap_or_default()
            } else {
                PrefabMetadata::default()
            };

            self.entries.insert(
                name.clone(),
                PrefabEntry {
                    name,
                    directory: path,
                    scene_path,
                    material_library: metadata.material_library,
                    particle_presets: metadata.particle_presets,
                },
            );
        }

        info!("Discovered {} prefabs", self.entries.len());
    }

    /// Save metadata sidecar for a prefab ({name}.scn.ron.meta)
    pub fn save_metadata(
        prefab_dir: &std::path::Path,
        prefab_name: &str,
        material_library: &MaterialLibrary,
        particle_presets: &HashMap<String, VfxSystem>,
    ) -> Result<(), String> {
        let metadata = PrefabMetadata {
            material_library: material_library.clone(),
            particle_presets: particle_presets.clone(),
        };

        let content = ron::ser::to_string_pretty(&metadata, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("Serialization error: {}", e))?;

        let meta_path = prefab_dir.join(format!("{}.scn.ron.meta", prefab_name));
        fs::write(&meta_path, content).map_err(|e| format!("Write error: {}", e))?;

        Ok(())
    }
}

/// System that scans the prefab directory at startup
pub fn scan_prefab_directory(mut registry: ResMut<PrefabRegistry>) {
    registry.refresh();
}
