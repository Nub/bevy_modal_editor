pub mod build;
pub mod data;

pub use data::*;

use bevy::prelude::*;
use bevy_hanabi::prelude::*;

use crate::scene::SceneEntity;

/// Marker on the child entity that holds the actual `ParticleEffect`.
/// The parent (container) has `ParticleEffectMarker` + `SceneEntity`;
/// this child is disposable and gets destroyed/recreated on every edit.
#[derive(Component)]
pub struct ParticleEffectChild;

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
