//! Default effect presets.

use bevy::prelude::*;

use super::data::*;

/// Return the built-in default effect presets as `(name, marker)` pairs.
pub fn default_presets() -> Vec<(&'static str, EffectMarker)> {
    vec![
        ("Falling Impact", falling_impact()),
        ("Pulsing Beacon", pulsing_beacon()),
    ]
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

/// A sphere spawns on start, scales up, then repeatedly pulses its scale using
/// tween chaining. Demonstrates OnSpawn, AfterRule, and TweenValue.
fn pulsing_beacon() -> EffectMarker {
    EffectMarker {
        steps: vec![
            EffectStep {
                name: "spawn_orb".into(),
                trigger: EffectTrigger::OnSpawn,
                actions: vec![
                    EffectAction::SpawnPrimitive {
                        tag: "orb".into(),
                        shape: crate::scene::PrimitiveShape::Sphere,
                        offset: Vec3::ZERO,
                        material: None,
                        rigid_body: None,
                    },
                    EffectAction::TweenValue {
                        target_tag: "orb".into(),
                        property: TweenProperty::Scale,
                        from: 0.1,
                        to: 1.0,
                        duration: 0.5,
                        easing: EasingType::EaseOut,
                    },
                ],
            },
            EffectStep {
                name: "pulse".into(),
                trigger: EffectTrigger::RepeatingInterval {
                    interval: 1.5,
                    max_count: None,
                },
                actions: vec![
                    EffectAction::TweenValue {
                        target_tag: "orb".into(),
                        property: TweenProperty::Scale,
                        from: 1.0,
                        to: 1.3,
                        duration: 0.4,
                        easing: EasingType::EaseInOut,
                    },
                ],
            },
            EffectStep {
                name: "pulse_back".into(),
                trigger: EffectTrigger::AfterRule {
                    source_rule: "pulse".into(),
                    delay: 0.4,
                },
                actions: vec![
                    EffectAction::TweenValue {
                        target_tag: "orb".into(),
                        property: TweenProperty::Scale,
                        from: 1.3,
                        to: 1.0,
                        duration: 0.4,
                        easing: EasingType::EaseInOut,
                    },
                ],
            },
        ],
    }
}
