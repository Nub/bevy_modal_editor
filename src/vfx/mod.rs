//! Editor integration for bevy_vfx.
//!
//! Handles preset disk I/O, rebuild bridge, and spawn event integration.

use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::*;
use bevy_vfx::{VfxLibrary, VfxSystem};

const VFX_DIR: &str = "assets/vfx";

pub struct VfxEditorPlugin;

impl Plugin for VfxEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy_vfx::VfxPlugin)
            .add_systems(PreStartup, init_vfx_library)
            .add_systems(Update, auto_save_vfx_presets);
    }
}

// ---------------------------------------------------------------------------
// Library initialization
// ---------------------------------------------------------------------------

fn init_vfx_library(mut library: ResMut<VfxLibrary>) {
    // Populate with built-in defaults
    for (name, system) in bevy_vfx::presets::default_presets() {
        library.effects.entry(name.to_string()).or_insert(system);
    }

    // Load disk overrides
    load_presets_from_disk(&mut library);
}

// ---------------------------------------------------------------------------
// Disk persistence
// ---------------------------------------------------------------------------

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect()
}

fn save_preset_to_disk(name: &str, system: &VfxSystem) {
    let dir = Path::new(VFX_DIR);
    if let Err(e) = std::fs::create_dir_all(dir) {
        warn!("Failed to create vfx directory: {}", e);
        return;
    }

    let filename = sanitize_filename(name);
    let path = dir.join(format!("{}.vfx.ron", filename));

    let pretty = ron::ser::PrettyConfig::default();
    match ron::ser::to_string_pretty(system, pretty) {
        Ok(ron_str) => {
            if let Err(e) = std::fs::write(&path, &ron_str) {
                warn!("Failed to write VFX preset '{}': {}", name, e);
            }
        }
        Err(e) => {
            warn!("Failed to serialize VFX preset '{}': {}", name, e);
        }
    }
}

fn load_presets_from_disk(library: &mut VfxLibrary) {
    let dir = Path::new(VFX_DIR);
    if !dir.is_dir() {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let fname = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("");
        if !fname.ends_with(".vfx.ron") {
            continue;
        }

        let name = fname.trim_end_matches(".vfx.ron").to_string();
        if name.is_empty() {
            continue;
        }

        let Ok(contents) = std::fs::read_to_string(&path) else {
            warn!("Failed to read VFX preset file: {:?}", path);
            continue;
        };

        match ron::from_str::<VfxSystem>(&contents) {
            Ok(system) => {
                library.effects.insert(name.clone(), system);
                info!("Loaded VFX preset '{}' from disk", name);
            }
            Err(e) => {
                warn!("Failed to parse VFX preset '{:?}': {}", path, e);
            }
        }
    }
}

fn auto_save_vfx_presets(
    library: Res<VfxLibrary>,
    mut prev_state: Local<HashMap<String, String>>,
) {
    if !library.is_changed() {
        return;
    }

    for (name, system) in &library.effects {
        let ron_str = ron::to_string(system).unwrap_or_default();
        let changed = match prev_state.get(name) {
            Some(prev) => prev != &ron_str,
            None => true,
        };
        if changed {
            save_preset_to_disk(name, system);
            prev_state.insert(name.clone(), ron_str);
        }
    }
}
