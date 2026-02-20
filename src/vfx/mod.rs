//! Editor integration for bevy_vfx.
//!
//! Handles preset disk I/O, rebuild bridge, and spawn event integration.

use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::*;
use bevy_editor_game::{MaterialLibrary, MeshLibrary};
use bevy_vfx::data::RenderModule;
use bevy_vfx::mesh_particles::{MeshParticleAssets, MeshParticleState, MeshShapeKey};
use bevy_vfx::{VfxLibrary, VfxSystem};

const VFX_DIR: &str = "assets/vfx";

pub struct VfxEditorPlugin;

impl Plugin for VfxEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy_vfx::VfxPlugin)
            .add_systems(PreStartup, init_vfx_library)
            .add_systems(Update, (auto_save_vfx_presets, sync_material_library_to_vfx, sync_mesh_library_to_vfx));
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

/// When MaterialLibrary changes, refresh cached material handles for mesh particles
/// and update all live particle children.
fn sync_material_library_to_vfx(
    library: Res<MaterialLibrary>,
    mut assets: ResMut<MeshParticleAssets>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    asset_server: Res<AssetServer>,
    mut particles: Query<(Entity, &mut MeshParticleState, &VfxSystem)>,
    mut commands: Commands,
) {
    if !library.is_changed() {
        return;
    }

    // Collect which named materials are in use by active mesh emitters
    let mut names_in_use: Vec<String> = Vec::new();
    for (_, _, system) in particles.iter() {
        for emitter in &system.emitters {
            if let RenderModule::Mesh(ref config) = emitter.render {
                if let Some(ref name) = config.material_path {
                    if !names_in_use.contains(name) {
                        names_in_use.push(name.clone());
                    }
                }
            }
        }
    }

    if names_in_use.is_empty() {
        return;
    }

    // Recreate handles for each in-use material
    for mat_name in &names_in_use {
        let Some(def) = library.materials.get(mat_name) else {
            continue;
        };

        let mut mat = def.base.to_standard_material();
        crate::materials::load_base_textures(&mut mat, &def.base, &asset_server);
        let handle = materials.add(mat);
        assets.named_materials.insert(mat_name.clone(), handle.clone());

        // Update all MeshParticleState instances that reference this material
        for (_, mut state, sys) in particles.iter_mut() {
            let Some(emitter) = sys.emitters.get(state.emitter_index) else {
                continue;
            };
            if let RenderModule::Mesh(ref config) = emitter.render {
                if config.material_path.as_deref() == Some(mat_name.as_str()) {
                    state.material_handle = Some(handle.clone());
                    for p in &state.particles {
                        commands.entity(p.entity).insert(MeshMaterial3d(handle.clone()));
                    }
                }
            }
        }
    }
}

/// When MeshLibrary changes, sync mesh handles into MeshParticleAssets
/// so Custom mesh shapes resolve to the correct mesh.
fn sync_mesh_library_to_vfx(
    mesh_library: Res<MeshLibrary>,
    mut assets: ResMut<MeshParticleAssets>,
) {
    if !mesh_library.is_changed() {
        return;
    }

    for (name, handle) in &mesh_library.meshes {
        assets.meshes.insert(MeshShapeKey::Custom(name.clone()), handle.clone());
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
