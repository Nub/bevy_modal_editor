//! GLTF Asset Library system.
//!
//! Loads GLTF files from `assets/libraries/` and from `GltfLibraryConfig`, then
//! indexes their contents into `MaterialLibrary`, `MeshLibrary`, `AnimationLibrary`,
//! and `SceneLibrary`.

use std::path::Path;

use bevy::gltf::{Gltf, GltfMesh};
use bevy::prelude::*;
use bevy_editor_game::{
    AnimationLibrary, BaseMaterialProps, GltfLibraryConfig, MaterialDefinition, MaterialLibrary,
    MeshLibrary, SceneLibrary,
};

/// Tracks the state of GLTF asset library loading.
#[derive(Resource)]
pub struct AssetLibraryState {
    /// GLTF handles being loaded, paired with a short name for namespacing.
    pending: Vec<(String, Handle<Gltf>)>,
    /// Total number of GLTF files queued for loading.
    pub total_count: usize,
    /// Number of GLTF files that have finished loading.
    pub loaded_count: usize,
    /// Whether initial scan + load has been kicked off.
    started: bool,
}

impl Default for AssetLibraryState {
    fn default() -> Self {
        Self {
            pending: Vec::new(),
            total_count: 0,
            loaded_count: 0,
            started: false,
        }
    }
}

impl AssetLibraryState {
    /// Returns true while there are still assets loading.
    pub fn is_loading(&self) -> bool {
        self.started && self.loaded_count < self.total_count
    }
}

pub struct AssetLibraryPlugin;

impl Plugin for AssetLibraryPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AssetLibraryState>()
            .init_resource::<GltfLibraryConfig>()
            .init_resource::<MeshLibrary>()
            .init_resource::<AnimationLibrary>()
            .init_resource::<SceneLibrary>()
            .add_systems(PreStartup, start_loading_libraries)
            .add_systems(Update, process_loaded_gltfs);
    }
}

/// Kick off loading of all GLTF libraries.
///
/// Sources:
/// 1. Files in `assets/libraries/*.glb` / `*.gltf`
/// 2. Paths registered via `GltfLibraryConfig`
fn start_loading_libraries(
    asset_server: Res<AssetServer>,
    config: Res<GltfLibraryConfig>,
    mut state: ResMut<AssetLibraryState>,
) {
    let mut paths: Vec<String> = Vec::new();

    // Scan assets/libraries/ directory
    let libraries_dir = Path::new("assets/libraries");
    if libraries_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(libraries_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if ext.eq_ignore_ascii_case("glb") || ext.eq_ignore_ascii_case("gltf") {
                        // Convert to asset-relative path (strip "assets/" prefix)
                        if let Ok(rel) = path.strip_prefix("assets") {
                            paths.push(rel.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    // Merge in config paths
    for path in &config.paths {
        if !paths.contains(path) {
            paths.push(path.clone());
        }
    }

    if paths.is_empty() {
        return;
    }

    info!("Asset libraries: loading {} GLTF files", paths.len());

    for path in paths {
        let short_name = Path::new(&path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        let handle: Handle<Gltf> = asset_server.load(path);
        state.pending.push((short_name, handle));
    }

    state.total_count = state.pending.len();
    state.loaded_count = 0;
    state.started = true;
}

/// Check pending GLTF handles and index their contents once loaded.
fn process_loaded_gltfs(
    mut state: ResMut<AssetLibraryState>,
    gltf_assets: Res<Assets<Gltf>>,
    gltf_mesh_assets: Res<Assets<GltfMesh>>,
    material_assets: Res<Assets<StandardMaterial>>,
    mut material_library: ResMut<MaterialLibrary>,
    mut mesh_library: ResMut<MeshLibrary>,
    mut animation_library: ResMut<AnimationLibrary>,
    mut scene_library: ResMut<SceneLibrary>,
) {
    if !state.started || state.pending.is_empty() {
        return;
    }

    // Drain into a local vec to avoid borrow issues
    let items: Vec<_> = state.pending.drain(..).collect();
    let mut still_pending = Vec::new();

    for (name, handle) in items {
        if let Some(gltf) = gltf_assets.get(&handle) {
            index_gltf(
                &name,
                gltf,
                &gltf_mesh_assets,
                &material_assets,
                &mut material_library,
                &mut mesh_library,
                &mut animation_library,
                &mut scene_library,
            );
            state.loaded_count += 1;
            info!(
                "Asset library indexed: {} ({}/{})",
                name, state.loaded_count, state.total_count
            );
        } else {
            still_pending.push((name, handle));
        }
    }

    state.pending = still_pending;
}

/// Index a single loaded GLTF into the various libraries.
fn index_gltf(
    gltf_name: &str,
    gltf: &Gltf,
    gltf_mesh_assets: &Assets<GltfMesh>,
    material_assets: &Assets<StandardMaterial>,
    material_library: &mut MaterialLibrary,
    mesh_library: &mut MeshLibrary,
    animation_library: &mut AnimationLibrary,
    scene_library: &mut SceneLibrary,
) {
    // --- Materials ---
    for (mat_name, mat_handle) in &gltf.named_materials {
        let key = format!("{}::{}", gltf_name, mat_name);
        if let Some(std_mat) = material_assets.get(mat_handle) {
            let props = BaseMaterialProps::from_standard_material(std_mat);
            material_library
                .materials
                .insert(key, MaterialDefinition::standard_from_props(props));
        }
    }
    // Also index unnamed materials by index
    for (idx, mat_handle) in gltf.materials.iter().enumerate() {
        let key = format!("{}::material_{}", gltf_name, idx);
        if material_library.materials.contains_key(&key) {
            continue; // Already indexed via named
        }
        if let Some(std_mat) = material_assets.get(mat_handle) {
            let props = BaseMaterialProps::from_standard_material(std_mat);
            material_library
                .materials
                .insert(key, MaterialDefinition::standard_from_props(props));
        }
    }

    // --- Meshes ---
    for (mesh_name, gltf_mesh_handle) in &gltf.named_meshes {
        if let Some(gltf_mesh) = gltf_mesh_assets.get(gltf_mesh_handle) {
            for (prim_idx, primitive) in gltf_mesh.primitives.iter().enumerate() {
                let key = if gltf_mesh.primitives.len() == 1 {
                    format!("{}::{}", gltf_name, mesh_name)
                } else {
                    format!("{}::{}_{}", gltf_name, mesh_name, prim_idx)
                };
                mesh_library.meshes.insert(key, primitive.mesh.clone());
            }
        }
    }
    // Also index unnamed meshes by index
    for (mesh_idx, gltf_mesh_handle) in gltf.meshes.iter().enumerate() {
        if let Some(gltf_mesh) = gltf_mesh_assets.get(gltf_mesh_handle) {
            for (prim_idx, primitive) in gltf_mesh.primitives.iter().enumerate() {
                let key = if gltf_mesh.primitives.len() == 1 {
                    format!("{}::mesh_{}", gltf_name, mesh_idx)
                } else {
                    format!("{}::mesh_{}_{}", gltf_name, mesh_idx, prim_idx)
                };
                if !mesh_library.meshes.contains_key(&key) {
                    mesh_library.meshes.insert(key, primitive.mesh.clone());
                }
            }
        }
    }

    // --- Animations ---
    for (anim_name, anim_handle) in &gltf.named_animations {
        let key = format!("{}::{}", gltf_name, anim_name);
        animation_library.clips.insert(key, anim_handle.clone());
    }
    for (idx, anim_handle) in gltf.animations.iter().enumerate() {
        let key = format!("{}::anim_{}", gltf_name, idx);
        if !animation_library.clips.contains_key(&key) {
            animation_library.clips.insert(key, anim_handle.clone());
        }
    }

    // --- Scenes ---
    for (scene_name, scene_handle) in &gltf.named_scenes {
        let key = format!("{}::{}", gltf_name, scene_name);
        scene_library.scenes.insert(key, scene_handle.clone());
    }
    for (idx, scene_handle) in gltf.scenes.iter().enumerate() {
        let key = format!("{}::scene_{}", gltf_name, idx);
        if !scene_library.scenes.contains_key(&key) {
            scene_library.scenes.insert(key, scene_handle.clone());
        }
    }
}
