//! Extract VfxSystem + Transform data from the main world into the render world.

use bevy::asset::AssetId;
use bevy::image::Image;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::render::Extract;

use crate::data::{EmitterDef, RenderModule, VfxSystem};

/// Extracted data for a single emitter, stored in a resource (not per-entity).
pub struct ExtractedEmitterInfo {
    /// Main-world entity that owns this emitter.
    pub source_entity: Entity,
    /// Index of this emitter within the parent VfxSystem.
    pub emitter_index: usize,
    /// Only the specific EmitterDef needed (avoids cloning entire VfxSystem).
    pub emitter: EmitterDef,
    /// World transform of the entity.
    pub transform: GlobalTransform,
    /// Texture asset ID for billboard rendering (None = procedural circle).
    pub texture: Option<AssetId<Image>>,
}

/// Resource holding all extracted emitter data for the current frame.
/// Rebuilt every frame during the extract phase.
#[derive(Resource, Default)]
pub struct ExtractedVfxData {
    pub emitters: Vec<ExtractedEmitterInfo>,
}

/// Main-world resource that holds strong handles to loaded VFX textures,
/// keeping them alive across frames. Paths are resolved via AssetServer.
#[derive(Resource, Default)]
pub struct VfxTextureCache {
    pub handles: HashMap<String, Handle<Image>>,
}

/// Main-world system: loads textures referenced by VfxSystem emitters.
pub fn load_vfx_textures(
    mut cache: ResMut<VfxTextureCache>,
    asset_server: Res<AssetServer>,
    query: Query<&VfxSystem>,
) {
    // Collect all referenced texture paths (owned) to avoid borrow conflicts
    let mut active_paths = bevy::platform::collections::HashSet::new();
    let mut to_load = Vec::new();
    for system in &query {
        for emitter in &system.emitters {
            if let RenderModule::Billboard(config) = &emitter.render {
                if let Some(path) = &config.texture {
                    active_paths.insert(path.clone());
                    if !cache.handles.contains_key(path) {
                        to_load.push(path.clone());
                    }
                }
            }
        }
    }
    for path in to_load {
        let handle = asset_server.load::<Image>(&path);
        cache.handles.insert(path, handle);
    }
    // Drop handles for textures no longer referenced by any emitter
    cache.handles.retain(|path, _| active_paths.contains(path));
}

/// Extract all VfxSystem entities from the main world into a resource.
pub fn extract_vfx_systems(
    mut extracted: ResMut<ExtractedVfxData>,
    query: Extract<Query<(Entity, &VfxSystem, &GlobalTransform)>>,
    texture_cache: Extract<Res<VfxTextureCache>>,
) {
    extracted.emitters.clear();

    for (entity, system, transform) in &query {
        for (idx, emitter) in system.emitters.iter().enumerate() {
            if !emitter.enabled {
                continue;
            }
            // Mesh emitters are simulated on the CPU — skip GPU extraction
            if matches!(emitter.render, RenderModule::Mesh(_)) {
                continue;
            }
            let texture = match &emitter.render {
                RenderModule::Billboard(config) => config.texture.as_ref().and_then(|path| {
                    texture_cache.handles.get(path).map(|h| h.id())
                }),
                _ => None,
            };
            extracted.emitters.push(ExtractedEmitterInfo {
                source_entity: entity,
                emitter_index: idx,
                emitter: emitter.clone(),
                transform: *transform,
                texture,
            });
        }
    }
}
