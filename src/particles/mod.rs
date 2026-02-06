pub mod build;
pub mod data;

pub use data::*;

use bevy::prelude::*;
use bevy_hanabi::prelude::*;

use crate::scene::SceneEntity;

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
            .add_systems(
                Update,
                rebuild_particle_effects.run_if(any_with_component::<ParticleEffectMarker>),
            );
    }
}

/// Detect changes to `ParticleEffectMarker` and rebuild the effect.
///
/// - Removes `ParticleEffect`, `EffectProperties`, and `CompiledParticleEffect`
///   then re-inserts a fresh `ParticleEffect` with the updated asset.
/// - Updates `EffectSpawner` settings directly for instant spawner changes.
fn rebuild_particle_effects(
    mut commands: Commands,
    mut effects: ResMut<Assets<EffectAsset>>,
    mut query: Query<
        (
            Entity,
            &ParticleEffectMarker,
            Option<&ParticleEffect>,
            Option<&mut EffectSpawner>,
        ),
        (With<SceneEntity>, Changed<ParticleEffectMarker>),
    >,
) {
    for (entity, marker, existing_effect, maybe_spawner) in &mut query {
        let asset = build::build_effect(marker);

        if let Some(_effect) = existing_effect {
            // Create a new asset and replace the ParticleEffect component,
            // which triggers hanabi to fully reinitialize.
            let handle = effects.add(asset);
            commands
                .entity(entity)
                .remove::<(ParticleEffect, EffectProperties, CompiledParticleEffect, EffectSpawner)>()
                .insert(ParticleEffect::new(handle));

            // Update spawner settings directly (instant)
            if let Some(mut spawner) = maybe_spawner {
                let new_settings = match &marker.spawner {
                    SpawnerConfig::Rate { rate } => SpawnerSettings::rate((*rate).into()),
                    SpawnerConfig::Once { count } => SpawnerSettings::once((*count).into()),
                    SpawnerConfig::Burst { count, period } => {
                        SpawnerSettings::burst((*count).into(), (*period).into())
                    }
                };
                spawner.settings = new_settings;
            }
        } else {
            // First time: create handle and insert ParticleEffect component
            let handle = effects.add(asset);
            commands.entity(entity).insert((
                ParticleEffect::new(handle),
                Visibility::default(),
            ));
        }
    }
}
