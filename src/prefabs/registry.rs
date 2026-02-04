use bevy::prelude::*;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use super::prefab::Prefab;

/// Resource containing all loaded prefabs
#[derive(Resource, Default)]
pub struct PrefabRegistry {
    pub prefabs: HashMap<String, Prefab>,
    pub prefab_directory: Option<PathBuf>,
}

impl PrefabRegistry {
    pub fn new() -> Self {
        Self {
            prefabs: HashMap::new(),
            prefab_directory: None,
        }
    }

    pub fn set_directory(&mut self, path: PathBuf) {
        self.prefab_directory = Some(path);
    }

    pub fn load_all(&mut self) {
        let Some(ref dir) = self.prefab_directory else {
            return;
        };

        if !dir.exists() {
            if let Err(e) = fs::create_dir_all(dir) {
                error!("Failed to create prefab directory: {}", e);
                return;
            }
        }

        let entries = match fs::read_dir(dir) {
            Ok(e) => e,
            Err(e) => {
                error!("Failed to read prefab directory: {}", e);
                return;
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("ron") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(prefab) = ron::from_str::<Prefab>(&content) {
                        self.prefabs.insert(prefab.name.clone(), prefab);
                    }
                }
            }
        }

        info!("Loaded {} prefabs", self.prefabs.len());
    }

    pub fn save_prefab(&self, prefab: &Prefab) -> Result<(), String> {
        let Some(ref dir) = self.prefab_directory else {
            return Err("No prefab directory set".to_string());
        };

        let path = dir.join(format!("{}.ron", prefab.name));
        let content = ron::ser::to_string_pretty(prefab, ron::ser::PrettyConfig::default())
            .map_err(|e| format!("Serialization error: {}", e))?;

        fs::write(&path, content).map_err(|e| format!("Write error: {}", e))?;

        info!("Saved prefab: {}", prefab.name);
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Prefab> {
        self.prefabs.get(name)
    }

    pub fn list(&self) -> Vec<&str> {
        self.prefabs.keys().map(|s| s.as_str()).collect()
    }
}

pub struct PrefabRegistryPlugin;

impl Plugin for PrefabRegistryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PrefabRegistry>()
            .add_systems(PreStartup, setup_prefab_registry);
    }
}

fn setup_prefab_registry(mut registry: ResMut<PrefabRegistry>) {
    // Set default prefab directory
    let prefab_dir = PathBuf::from("assets/prefabs");
    registry.set_directory(prefab_dir);
    registry.load_all();
}
