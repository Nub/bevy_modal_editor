//! Default effect presets.

use bevy::prelude::*;

use super::data::*;

/// Return the built-in default effect presets as `(name, marker)` pairs.
pub fn default_presets() -> Vec<(&'static str, EffectMarker)> {
    vec![("Falling Impact", falling_impact())]
}

/// A GLTF model spawned at height falls under gravity; on collision, spawns dust
/// particles, sparks, a ground crack decal, then emits an "impact" event.
fn falling_impact() -> EffectMarker {
    EffectMarker {
        steps: vec![
            EffectStep {
                name: "spawn_anchor".into(),
                trigger: EffectTrigger::AtTime(0.0),
                actions: vec![
                    EffectAction::SpawnGltf {
                        tag: "anchor".into(),
                        path: "objects/Duck.glb".into(),
                        at: SpawnLocation::Offset(Vec3::new(0.0, 5.0, 0.0)),
                        scale: Vec3::splat(1.0),
                        rigid_body: Some(RigidBodyKind::Dynamic),
                    },
                    EffectAction::SetVelocity {
                        tag: "anchor".into(),
                        velocity: Vec3::new(0.0, -8.0, 0.0),
                    },
                ],
            },
            EffectStep {
                name: "impact".into(),
                trigger: EffectTrigger::OnCollision {
                    tag: "anchor".into(),
                },
                actions: vec![
                    EffectAction::SpawnParticle {
                        tag: "dust".into(),
                        preset: "Smoke".into(),
                        at: SpawnLocation::CollisionPoint,
                    },
                    EffectAction::SpawnParticle {
                        tag: "sparks".into(),
                        preset: "Sparks".into(),
                        at: SpawnLocation::CollisionPoint,
                    },
                    EffectAction::SpawnDecal {
                        tag: "crack".into(),
                        texture_path: "textures/decal_splat.png".into(),
                        at: SpawnLocation::CollisionPoint,
                        scale: Vec3::splat(2.0),
                    },
                    EffectAction::EmitEvent("impact".into()),
                ],
            },
        ],
    }
}
