//! Editor integration for bevy_vfx.
//!
//! Handles preset disk I/O, rebuild bridge, and spawn event integration.

use std::collections::HashMap;
use std::path::Path;

use bevy::math::Affine2;
use bevy::prelude::*;
use bevy_editor_game::{MaterialLibrary, MeshLibrary};
use bevy_vfx::data::{InitModule, RenderModule, UpdateModule};
use bevy_vfx::mesh_particles::{MeshParticleAssets, MeshParticleStates, MeshShapeKey, VfxMaterialPending};
use bevy_vfx::{VfxLibrary, VfxSystem};

use crate::materials::{apply_material_def_standalone, remove_all_material_components, resolve_material_ref, set_entity_base_color, set_entity_emissive, set_entity_uv_transform};

const VFX_DIR: &str = "assets/vfx";

pub struct VfxEditorPlugin;

impl Plugin for VfxEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(bevy_vfx::VfxPlugin)
            .add_systems(PreStartup, init_vfx_library)
            .add_systems(Update, (auto_save_vfx_presets, sync_mesh_library_to_vfx))
            .add_systems(Update, apply_pending_vfx_materials)
            .add_systems(Update, sync_material_library_to_vfx)
            .add_systems(Update, vfx_library_material_uv_scroll)
            .add_systems(Update, vfx_library_material_color_sync);
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

/// Apply library materials to newly spawned mesh particle children.
///
/// Uses the same `apply_material_def_standalone` pipeline as the material editor,
/// so custom shader extensions (Grid, Channel Threshold, etc.) are applied correctly.
fn apply_pending_vfx_materials(world: &mut World) {
    // Collect pending entities
    let pending: Vec<(Entity, String)> = world
        .query::<(Entity, &VfxMaterialPending)>()
        .iter(world)
        .map(|(e, p)| (e, p.0.clone()))
        .collect();

    if pending.is_empty() {
        return;
    }

    for (entity, mat_name) in &pending {
        // Look up the material ref in the library
        let mat_ref = bevy_editor_game::MaterialRef::Library(mat_name.clone());
        let def = {
            let library = world.resource::<MaterialLibrary>();
            resolve_material_ref(&mat_ref, library).cloned()
        };

        if let Some(def) = def {
            // Remove any existing material components before applying the new one
            remove_all_material_components(world, *entity);
            apply_material_def_standalone(world, *entity, &def);
        }

        // Remove the pending marker
        if let Ok(mut e) = world.get_entity_mut(*entity) {
            e.remove::<VfxMaterialPending>();
        }
    }
}

/// When MaterialLibrary changes, re-apply materials to all live mesh particle children
/// that use a named library material.
fn sync_material_library_to_vfx(world: &mut World) {
    if !world.is_resource_changed::<MaterialLibrary>() {
        return;
    }

    // Collect particle entities that use named library materials
    let mut entity_materials: Vec<(Entity, String)> = Vec::new();
    let mut state_query = world.query::<(&MeshParticleStates, &VfxSystem)>();
    for (states, sys) in state_query.iter(world) {
        for state in &states.entries {
            let Some(emitter) = sys.emitters.get(state.emitter_index) else {
                continue;
            };
            if let RenderModule::Mesh(ref config) = emitter.render {
                if let Some(ref mat_name) = config.material_path {
                    for p in &state.particles {
                        entity_materials.push((p.entity, mat_name.clone()));
                    }
                }
            }
        }
    }

    if entity_materials.is_empty() {
        return;
    }

    // Apply the library material to each particle entity
    for (entity, mat_name) in &entity_materials {
        let mat_ref = bevy_editor_game::MaterialRef::Library(mat_name.clone());
        let def = {
            let library = world.resource::<MaterialLibrary>();
            resolve_material_ref(&mat_ref, library).cloned()
        };

        if let Some(def) = def {
            remove_all_material_components(world, *entity);
            apply_material_def_standalone(world, *entity, &def);
        }
    }
}

/// UV scroll for mesh particles using library materials (extended material types).
/// Each particle's UV offset is based on its individual age (time since spawn).
fn vfx_library_material_uv_scroll(world: &mut World) {
    // Collect per-particle scroll data for library material particles
    struct ParticleScroll {
        entity: Entity,
        uv: Affine2,
    }

    let mut updates: Vec<ParticleScroll> = Vec::new();

    let mut query = world.query::<(&MeshParticleStates, &VfxSystem)>();
    for (states, sys) in query.iter(world) {
        for state in &states.entries {
            let Some(emitter) = sys.emitters.get(state.emitter_index) else {
                continue;
            };
            let RenderModule::Mesh(ref config) = emitter.render else {
                continue;
            };
            if config.material_path.is_none() {
                continue;
            }

            let scroll_speed = emitter
                .update
                .iter()
                .find_map(|m| match m {
                    UpdateModule::UvScroll { speed } => Some(Vec2::new(speed[0], speed[1])),
                    _ => None,
                })
                .unwrap_or(Vec2::ZERO);

            if scroll_speed == Vec2::ZERO {
                continue;
            }

            let uv_scale = emitter
                .init
                .iter()
                .find_map(|m| match m {
                    InitModule::SetUvScale(s) => Some(Vec2::new(s[0], s[1])),
                    _ => None,
                })
                .unwrap_or(Vec2::ONE);

            for p in &state.particles {
                let offset = scroll_speed * p.age;
                updates.push(ParticleScroll {
                    entity: p.entity,
                    uv: Affine2::from_scale_angle_translation(uv_scale, 0.0, offset),
                });
            }
        }
    }

    for ps in &updates {
        set_entity_uv_transform(world, ps.entity, ps.uv);
    }
}

/// Sync ColorByLife and EmissiveOverLife to mesh particles using library materials.
/// Uses type-erased material setters so it works with any material type (Standard, Channel Threshold, etc.).
fn vfx_library_material_color_sync(world: &mut World) {
    struct ParticleColor {
        entity: Entity,
        base_color: Option<Color>,
        emissive: Option<LinearRgba>,
    }

    let mut updates: Vec<ParticleColor> = Vec::new();

    let mut query = world.query::<(&MeshParticleStates, &VfxSystem)>();
    for (states, sys) in query.iter(world) {
        for state in &states.entries {
            let Some(emitter) = sys.emitters.get(state.emitter_index) else {
                continue;
            };
            let RenderModule::Mesh(ref config) = emitter.render else {
                continue;
            };
            if config.material_path.is_none() {
                continue;
            }

            let has_color_by_life = emitter
                .update
                .iter()
                .any(|m| matches!(m, UpdateModule::ColorByLife(_)));
            let has_emissive_by_life = emitter
                .update
                .iter()
                .any(|m| matches!(m, UpdateModule::EmissiveOverLife(_)));

            if !has_color_by_life && !has_emissive_by_life {
                continue;
            }

            for p in &state.particles {
                updates.push(ParticleColor {
                    entity: p.entity,
                    base_color: if has_color_by_life {
                        Some(Color::LinearRgba(p.color))
                    } else {
                        None
                    },
                    emissive: if has_emissive_by_life {
                        Some(p.emissive)
                    } else {
                        None
                    },
                });
            }
        }
    }

    for pc in &updates {
        if let Some(color) = pc.base_color {
            set_entity_base_color(world, pc.entity, color);
        }
        if let Some(emissive) = pc.emissive {
            set_entity_emissive(world, pc.entity, emissive);
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
