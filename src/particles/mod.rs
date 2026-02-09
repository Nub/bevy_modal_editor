pub mod build;
pub mod data;
pub mod presets;

pub use data::*;

use std::collections::HashMap;
use std::path::Path;

use bevy::prelude::*;
use bevy_hanabi::prelude::*;

use crate::scene::SceneEntity;

/// Marker on the child entity that holds the actual `ParticleEffect`.
/// The parent (container) has `ParticleEffectMarker` + `SceneEntity`;
/// this child is disposable and gets destroyed/recreated on every edit.
#[derive(Component)]
pub struct ParticleEffectChild;

/// Library of named particle effect presets.
#[derive(Resource, Default)]
pub struct ParticleLibrary {
    pub effects: HashMap<String, ParticleEffectMarker>,
}

const PARTICLES_DIR: &str = "assets/particles";

pub struct ParticlePlugin;

impl Plugin for ParticlePlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<ParticleEffectMarker>()
            .register_type::<SpawnerConfig>()
            .register_type::<ParticleSimSpace>()
            .register_type::<ParticleSimCondition>()
            .register_type::<ParticleMotionIntegration>()
            .register_type::<ParticleAlphaMode>()
            .register_type::<ParticleOrientMode>()
            .register_type::<ScalarRange>()
            .register_type::<GradientKeyData>()
            .register_type::<InitModifierData>()
            .register_type::<UpdateModifierData>()
            .register_type::<AccelModifierData>()
            .register_type::<RadialAccelData>()
            .register_type::<LinearDragData>()
            .register_type::<KillAabbData>()
            .register_type::<KillSphereData>()
            .register_type::<RenderModifierData>()
            .init_resource::<ParticleLibrary>()
            .add_systems(PreStartup, init_particle_library)
            .add_systems(
                Update,
                (
                    rebuild_particle_effects.run_if(any_with_component::<ParticleEffectMarker>),
                    auto_save_particle_presets,
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// Library initialization
// ---------------------------------------------------------------------------

fn init_particle_library(mut library: ResMut<ParticleLibrary>) {
    // Populate with built-in defaults
    for (name, marker) in presets::default_presets() {
        library.effects.entry(name.to_string()).or_insert(marker);
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

fn save_preset_to_disk(name: &str, marker: &ParticleEffectMarker) {
    let dir = Path::new(PARTICLES_DIR);
    if let Err(e) = std::fs::create_dir_all(dir) {
        warn!("Failed to create particles directory: {}", e);
        return;
    }

    let filename = sanitize_filename(name);
    let path = dir.join(format!("{}.pfx.ron", filename));

    let pretty = ron::ser::PrettyConfig::default();
    match ron::ser::to_string_pretty(marker, pretty) {
        Ok(ron_str) => {
            if let Err(e) = std::fs::write(&path, &ron_str) {
                warn!("Failed to write particle preset '{}': {}", name, e);
            }
        }
        Err(e) => {
            warn!("Failed to serialize particle preset '{}': {}", name, e);
        }
    }
}

fn load_presets_from_disk(library: &mut ParticleLibrary) {
    let dir = Path::new(PARTICLES_DIR);
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
        if !fname.ends_with(".pfx.ron") {
            continue;
        }

        let name = fname.trim_end_matches(".pfx.ron").to_string();
        if name.is_empty() {
            continue;
        }

        let Ok(contents) = std::fs::read_to_string(&path) else {
            warn!("Failed to read particle preset file: {:?}", path);
            continue;
        };

        match ron::from_str::<ParticleEffectMarker>(&contents) {
            Ok(marker) => {
                library.effects.insert(name.clone(), marker);
                info!("Loaded particle preset '{}' from disk", name);
            }
            Err(e) => {
                warn!("Failed to parse particle preset '{:?}': {}", path, e);
            }
        }
    }
}

fn auto_save_particle_presets(
    library: Res<ParticleLibrary>,
    mut prev_state: Local<HashMap<String, String>>,
) {
    if !library.is_changed() {
        return;
    }

    for (name, marker) in &library.effects {
        let ron_str = ron::to_string(marker).unwrap_or_default();
        let changed = match prev_state.get(name) {
            Some(prev) => prev != &ron_str,
            None => true,
        };
        if changed {
            save_preset_to_disk(name, marker);
            prev_state.insert(name.clone(), ron_str);
        }
    }
}

// ---------------------------------------------------------------------------
// Effect rebuild system
// ---------------------------------------------------------------------------

/// Detect changes to `ParticleEffectMarker` and rebuild the effect.
///
/// The container entity (with `SceneEntity` + `ParticleEffectMarker`) never
/// gets hanabi components. Instead, a disposable child entity holds the
/// `ParticleEffect`. On every edit the old child is despawned and a fresh
/// one is spawned with a new asset.
fn rebuild_particle_effects(
    mut commands: Commands,
    mut effects: ResMut<Assets<EffectAsset>>,
    query: Query<
        (Entity, &ParticleEffectMarker, Option<&Children>),
        (With<SceneEntity>, Changed<ParticleEffectMarker>),
    >,
    effect_children: Query<Entity, With<ParticleEffectChild>>,
) {
    for (container, marker, children) in &query {
        // Despawn any existing effect child
        if let Some(children) = children {
            for child in children.iter() {
                if effect_children.contains(child) {
                    commands.entity(child).despawn();
                }
            }
        }

        // Build a fresh asset and spawn a new child
        let asset = build::build_effect(marker);
        let handle = effects.add(asset);

        let child = commands
            .spawn((
                ParticleEffectChild,
                ParticleEffect::new(handle),
            ))
            .id();

        commands.entity(container).add_child(child);
    }
}
