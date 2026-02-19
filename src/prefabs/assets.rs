use bevy::prelude::*;
use bevy_editor_game::{BaseMaterialProps, MaterialLibrary, MaterialRef};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::scene::{GltfSource, SceneSource};

/// Resolve a relative prefab asset path to an absolute path.
pub fn resolve_prefab_asset_path(prefab_dir: &Path, relative_path: &str) -> PathBuf {
    prefab_dir.join(relative_path)
}

/// Collect all asset paths referenced by a set of entities.
/// Returns paths from GltfSource, SceneSource, and material textures.
pub fn collect_asset_paths(
    world: &World,
    entities: &[Entity],
    material_library: &MaterialLibrary,
) -> Vec<String> {
    let mut paths = Vec::new();

    for &entity in entities {
        // GltfSource paths
        if let Some(gltf) = world.entity(entity).get::<GltfSource>() {
            paths.push(gltf.path.clone());
        }

        // SceneSource paths
        if let Some(scene) = world.entity(entity).get::<SceneSource>() {
            paths.push(scene.path.clone());
        }

        // Material texture paths
        if let Some(mat_ref) = world.entity(entity).get::<MaterialRef>() {
            match mat_ref {
                MaterialRef::Library(name) => {
                    if let Some(def) = material_library.materials.get(name) {
                        collect_texture_paths(&def.base, &mut paths);
                    }
                }
                MaterialRef::Inline(def) => {
                    collect_texture_paths(&def.base, &mut paths);
                }
            }
        }
    }

    paths.sort();
    paths.dedup();
    paths
}

/// Extract texture paths from a material's base properties.
fn collect_texture_paths(base: &BaseMaterialProps, paths: &mut Vec<String>) {
    if let Some(ref p) = base.base_color_texture {
        paths.push(p.clone());
    }
    if let Some(ref p) = base.normal_map_texture {
        paths.push(p.clone());
    }
    if let Some(ref p) = base.metallic_roughness_texture {
        paths.push(p.clone());
    }
    if let Some(ref p) = base.emissive_texture {
        paths.push(p.clone());
    }
    if let Some(ref p) = base.occlusion_texture {
        paths.push(p.clone());
    }
    if let Some(ref p) = base.depth_map_texture {
        paths.push(p.clone());
    }
}

/// Determine the appropriate subdirectory for an asset path based on extension.
fn asset_subdir(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.ends_with(".glb") || lower.ends_with(".gltf") || lower.ends_with(".obj") {
        "models"
    } else if lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".hdr")
        || lower.ends_with(".exr")
        || lower.ends_with(".ktx2")
    {
        "textures"
    } else if lower.ends_with(".scene") || lower.ends_with(".ron") {
        "scenes"
    } else {
        "assets"
    }
}

/// Copy referenced assets into the prefab directory and return a path remapping table.
/// Keys are original paths, values are new paths relative to the prefab directory.
pub fn bundle_assets_into_prefab(
    asset_paths: &[String],
    prefab_dir: &Path,
) -> HashMap<String, String> {
    let mut remap = HashMap::new();

    for original_path in asset_paths {
        let subdir = asset_subdir(original_path);
        let target_dir = prefab_dir.join(subdir);

        if let Err(e) = fs::create_dir_all(&target_dir) {
            warn!("Failed to create directory {:?}: {}", target_dir, e);
            continue;
        }

        // Source is relative to assets/ or could be absolute
        let source = if Path::new(original_path).is_absolute() {
            PathBuf::from(original_path)
        } else {
            PathBuf::from("assets").join(original_path)
        };

        let filename = Path::new(original_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let target = target_dir.join(filename);
        let relative = format!("{}/{}", subdir, filename);

        if source.exists() {
            if let Err(e) = fs::copy(&source, &target) {
                warn!("Failed to copy {:?} → {:?}: {}", source, target, e);
                continue;
            }
        } else {
            warn!("Asset not found for bundling: {:?}", source);
        }

        remap.insert(original_path.clone(), relative);
    }

    remap
}

/// Remap asset paths in a material library using the given remapping table.
pub fn remap_material_library(
    library: &mut MaterialLibrary,
    remap: &HashMap<String, String>,
) {
    for def in library.materials.values_mut() {
        remap_material_textures(&mut def.base, remap);
    }
}

/// Remap texture paths in a material definition.
fn remap_material_textures(base: &mut BaseMaterialProps, remap: &HashMap<String, String>) {
    remap_path_option(&mut base.base_color_texture, remap);
    remap_path_option(&mut base.normal_map_texture, remap);
    remap_path_option(&mut base.metallic_roughness_texture, remap);
    remap_path_option(&mut base.emissive_texture, remap);
    remap_path_option(&mut base.occlusion_texture, remap);
    remap_path_option(&mut base.depth_map_texture, remap);
}

fn remap_path_option(path: &mut Option<String>, remap: &HashMap<String, String>) {
    if let Some(original) = path.as_ref() {
        if let Some(new_path) = remap.get(original) {
            *path = Some(new_path.clone());
        }
    }
}

/// Remap a MaterialRef's inline texture paths.
#[allow(dead_code)]
pub fn remap_material_ref(mat_ref: &mut MaterialRef, remap: &HashMap<String, String>) {
    match mat_ref {
        MaterialRef::Inline(def) => {
            remap_material_textures(&mut def.base, remap);
        }
        MaterialRef::Library(_) => {} // Library refs stay as names
    }
}
