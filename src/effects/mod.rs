pub mod data;
pub mod presets;

pub use data::*;

use std::collections::HashMap;
use std::path::Path;

use avian3d::prelude::*;
use bevy::prelude::*;

use crate::constants::physics;
use crate::particles::ParticleLibrary;

/// Library of named effect presets.
#[derive(Resource, Default)]
pub struct EffectLibrary {
    pub effects: HashMap<String, EffectMarker>,
}

const EFFECTS_DIR: &str = "assets/effects";

pub struct EffectPlugin;

impl Plugin for EffectPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<EffectMarker>()
            .register_type::<EffectStep>()
            .register_type::<EffectTrigger>()
            .register_type::<EffectAction>()
            .register_type::<RigidBodyKind>()
            .register_type::<SpawnLocation>()
            .init_resource::<EffectLibrary>()
            .add_systems(PreStartup, init_effect_library)
            .add_systems(
                Update,
                (
                    rebuild_effect_playback.run_if(any_with_component::<EffectMarker>),
                    detect_effect_collisions
                        .before(advance_effects)
                        .run_if(any_with_component::<EffectPlayback>),
                    advance_effects.run_if(any_with_component::<EffectPlayback>),
                    auto_save_effect_presets,
                ),
            );
    }
}

// ---------------------------------------------------------------------------
// Library initialization
// ---------------------------------------------------------------------------

fn init_effect_library(mut library: ResMut<EffectLibrary>) {
    for (name, marker) in presets::default_presets() {
        library.effects.entry(name.to_string()).or_insert(marker);
    }
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

fn save_preset_to_disk(name: &str, marker: &EffectMarker) {
    let dir = Path::new(EFFECTS_DIR);
    if let Err(e) = std::fs::create_dir_all(dir) {
        warn!("Failed to create effects directory: {}", e);
        return;
    }

    let filename = sanitize_filename(name);
    let path = dir.join(format!("{}.fx.ron", filename));

    let pretty = ron::ser::PrettyConfig::default();
    match ron::ser::to_string_pretty(marker, pretty) {
        Ok(ron_str) => {
            if let Err(e) = std::fs::write(&path, &ron_str) {
                warn!("Failed to write effect preset '{}': {}", name, e);
            }
        }
        Err(e) => {
            warn!("Failed to serialize effect preset '{}': {}", name, e);
        }
    }
}

fn load_presets_from_disk(library: &mut EffectLibrary) {
    let dir = Path::new(EFFECTS_DIR);
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
        if !fname.ends_with(".fx.ron") {
            continue;
        }

        let name = fname.trim_end_matches(".fx.ron").to_string();
        if name.is_empty() {
            continue;
        }

        let Ok(contents) = std::fs::read_to_string(&path) else {
            warn!("Failed to read effect preset file: {:?}", path);
            continue;
        };

        match ron::from_str::<EffectMarker>(&contents) {
            Ok(marker) => {
                library.effects.insert(name.clone(), marker);
                info!("Loaded effect preset '{}' from disk", name);
            }
            Err(e) => {
                warn!("Failed to parse effect preset '{:?}': {}", path, e);
            }
        }
    }
}

fn auto_save_effect_presets(
    library: Res<EffectLibrary>,
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
// Playback rebuild
// ---------------------------------------------------------------------------

/// Insert `EffectPlayback` on any `EffectMarker` entity that doesn't have one.
fn rebuild_effect_playback(
    mut commands: Commands,
    query: Query<Entity, (With<EffectMarker>, Without<EffectPlayback>)>,
) {
    for entity in &query {
        commands.entity(entity).insert(EffectPlayback::default());
    }
}

// ---------------------------------------------------------------------------
// Collision detection for effect children
// ---------------------------------------------------------------------------

/// Check `Collisions` for entities spawned by effects and populate
/// `EffectPlayback::collision_tags` so `OnCollision` triggers can fire.
fn detect_effect_collisions(
    collisions: Collisions,
    effect_children: Query<(Entity, &EffectChild)>,
    mut effects: Query<&mut EffectPlayback>,
) {
    for (child_entity, child) in &effect_children {
        // Check if this child is currently colliding with anything
        let mut hit_point = None;
        for contact_pair in collisions.collisions_with(child_entity) {
            if !contact_pair.is_touching() {
                continue;
            }
            // Grab the first contact point as the collision location
            if hit_point.is_none() {
                for manifold in &contact_pair.manifolds {
                    if let Some(cp) = manifold.points.first() {
                        hit_point = Some(cp.point);
                        break;
                    }
                }
            }
            break;
        }

        if hit_point.is_some() {
            if let Ok(mut playback) = effects.get_mut(child.effect_entity) {
                if playback.state == PlaybackState::Playing {
                    playback.collision_tags.insert(child.tag.clone());
                    if let Some(pt) = hit_point {
                        playback.last_collision_point = Some(pt);
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Playback advance
// ---------------------------------------------------------------------------

/// Main playback tick: advance time, check triggers, execute actions.
fn advance_effects(
    mut commands: Commands,
    time: Res<Time>,
    mut effects: Query<(Entity, &EffectMarker, &mut EffectPlayback, &GlobalTransform)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    particle_library: Res<ParticleLibrary>,
) {
    for (effect_entity, marker, mut playback, effect_transform) in &mut effects {
        if playback.state != PlaybackState::Playing {
            continue;
        }

        playback.elapsed += time.delta_secs();

        // Collect events emitted this frame for cross-step triggering
        let mut new_events: Vec<String> = Vec::new();

        for (step_idx, step) in marker.steps.iter().enumerate() {
            if playback.fired_steps.contains(&step_idx) {
                continue;
            }

            let should_fire = match &step.trigger {
                EffectTrigger::AtTime(t) => playback.elapsed >= *t,
                EffectTrigger::OnCollision { tag } => playback.collision_tags.contains(tag),
                EffectTrigger::OnEffectEvent(name) => playback.pending_events.contains(name),
            };

            if !should_fire {
                continue;
            }

            playback.fired_steps.insert(step_idx);

            for action in &step.actions {
                execute_action(
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    &particle_library,
                    effect_entity,
                    effect_transform,
                    &mut playback,
                    action,
                    &mut new_events,
                );
            }
        }

        // Merge new events so subsequent steps can see them next frame
        playback.pending_events.extend(new_events);
    }

    // Clear pending events and collision tags at end of frame (they only live one tick)
    for (_, _, mut playback, _) in &mut effects {
        if playback.state == PlaybackState::Playing {
            playback.pending_events.clear();
            playback.collision_tags.clear();
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn execute_action(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    particle_library: &ParticleLibrary,
    effect_entity: Entity,
    effect_transform: &GlobalTransform,
    playback: &mut EffectPlayback,
    action: &EffectAction,
    new_events: &mut Vec<String>,
) {
    match action {
        EffectAction::SpawnPrimitive {
            tag,
            shape,
            offset,
            material: _,
            rigid_body,
        } => {
            let world_pos = effect_transform.translation() + *offset;
            let mesh_handle = meshes.add(shape.create_mesh());
            let mat_handle = materials.add(shape.create_material());
            let collider = shape.create_collider();

            let mut entity_cmds = commands.spawn((
                EffectChild {
                    effect_entity,
                    tag: tag.clone(),
                },
                Mesh3d(mesh_handle),
                MeshMaterial3d(mat_handle),
                Transform::from_translation(world_pos),
                collider,
            ));

            if let Some(rb_kind) = rigid_body {
                entity_cmds.insert(rb_kind.to_rigid_body());
            }

            let child = entity_cmds.id();
            playback.spawned.insert(tag.clone(), child);
        }
        EffectAction::SpawnParticle { tag, preset, at } => {
            let pos = match at {
                SpawnLocation::Offset(offset) => {
                    effect_transform.translation() + *offset
                }
                SpawnLocation::CollisionPoint => {
                    playback
                        .last_collision_point
                        .unwrap_or_else(|| effect_transform.translation())
                }
            };

            let marker = particle_library
                .effects
                .get(preset)
                .cloned()
                .unwrap_or_default();

            let child = commands
                .spawn((
                    EffectChild {
                        effect_entity,
                        tag: tag.clone(),
                    },
                    marker,
                    Transform::from_translation(pos),
                    Visibility::default(),
                    Collider::sphere(physics::LIGHT_COLLIDER_RADIUS),
                ))
                .id();

            playback.spawned.insert(tag.clone(), child);
        }
        EffectAction::SetVelocity { tag, velocity } => {
            if let Some(&child_entity) = playback.spawned.get(tag) {
                commands
                    .entity(child_entity)
                    .insert(LinearVelocity(*velocity));
            }
        }
        EffectAction::ApplyImpulse { tag, impulse } => {
            if let Some(&child_entity) = playback.spawned.get(tag) {
                // Avian3D 0.5 has no ExternalImpulse; insert LinearVelocity directly
                commands
                    .entity(child_entity)
                    .insert(LinearVelocity(*impulse));
            }
        }
        EffectAction::Despawn { tag } => {
            if let Some(child_entity) = playback.spawned.remove(tag) {
                commands.entity(child_entity).despawn();
            }
        }
        EffectAction::EmitEvent(name) => {
            new_events.push(name.clone());
        }
        EffectAction::SetGravity { tag, enabled } => {
            if let Some(&child_entity) = playback.spawned.get(tag) {
                if *enabled {
                    commands
                        .entity(child_entity)
                        .insert(GravityScale(1.0));
                } else {
                    commands
                        .entity(child_entity)
                        .insert(GravityScale(0.0));
                }
            }
        }
    }
}

/// Despawn all children belonging to an effect and reset its playback state.
pub fn cleanup_effect(commands: &mut Commands, playback: &mut EffectPlayback) {
    for (_tag, entity) in playback.spawned.drain() {
        commands.entity(entity).despawn();
    }
    playback.elapsed = 0.0;
    playback.fired_steps.clear();
    playback.pending_events.clear();
    playback.collision_tags.clear();
    playback.last_collision_point = None;
    playback.state = PlaybackState::Stopped;
}
