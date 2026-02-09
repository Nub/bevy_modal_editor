//! Default particle effect presets.

use bevy::prelude::*;

use super::data::*;

/// Return the built-in default particle presets as `(name, marker)` pairs.
pub fn default_presets() -> Vec<(&'static str, ParticleEffectMarker)> {
    vec![
        ("Fire", fire()),
        ("Smoke", smoke()),
        ("Sparks", sparks()),
        ("Fountain", fountain()),
        ("Magic Orb", magic_orb()),
        ("Snow", snow()),
    ]
}

fn fire() -> ParticleEffectMarker {
    ParticleEffectMarker {
        capacity: 512,
        spawner: SpawnerConfig::Rate { rate: 80.0 },
        simulation_space: ParticleSimSpace::Global,
        simulation_condition: ParticleSimCondition::Always,
        motion_integration: ParticleMotionIntegration::PostUpdate,
        alpha_mode: ParticleAlphaMode::Add,
        init_modifiers: vec![
            InitModifierData::SetLifetime(ScalarRange::Random(0.4, 1.2)),
            InitModifierData::SetPositionSphere {
                center: Vec3::ZERO,
                radius: ScalarRange::Random(0.05, 0.3),
                volume: true,
            },
            InitModifierData::SetVelocitySphere {
                center: Vec3::ZERO,
                speed: ScalarRange::Random(0.8, 2.5),
            },
        ],
        update_modifiers: vec![
            AccelModifierData {
                accel: Vec3::new(0.0, 3.0, 0.0),
            }
            .into(),
            UpdateModifierData::LinearDrag(LinearDragData { drag: 1.5 }),
        ],
        render_modifiers: vec![
            RenderModifierData::ParticleTexture {
                path: Some("textures/particles/soft_circle.png".into()),
                sample_mapping: ParticleSampleMapping::Modulate,
            },
            RenderModifierData::ColorOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(1.0, 0.9, 0.3, 1.0),
                    },
                    GradientKeyData {
                        ratio: 0.3,
                        value: Vec4::new(1.0, 0.5, 0.1, 0.9),
                    },
                    GradientKeyData {
                        ratio: 0.7,
                        value: Vec4::new(0.8, 0.15, 0.0, 0.5),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.3, 0.05, 0.0, 0.0),
                    },
                ],
            },
            RenderModifierData::SizeOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(0.15, 0.15, 0.15, 0.0),
                    },
                    GradientKeyData {
                        ratio: 0.5,
                        value: Vec4::new(0.25, 0.25, 0.25, 0.0),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.05, 0.05, 0.05, 0.0),
                    },
                ],
            },
        ],
    }
}

fn smoke() -> ParticleEffectMarker {
    ParticleEffectMarker {
        capacity: 256,
        spawner: SpawnerConfig::Rate { rate: 20.0 },
        simulation_space: ParticleSimSpace::Global,
        simulation_condition: ParticleSimCondition::Always,
        motion_integration: ParticleMotionIntegration::PostUpdate,
        alpha_mode: ParticleAlphaMode::Blend,
        init_modifiers: vec![
            InitModifierData::SetLifetime(ScalarRange::Random(2.0, 4.0)),
            InitModifierData::SetPositionSphere {
                center: Vec3::ZERO,
                radius: ScalarRange::Random(0.1, 0.4),
                volume: true,
            },
            InitModifierData::SetVelocitySphere {
                center: Vec3::ZERO,
                speed: ScalarRange::Random(0.3, 0.8),
            },
        ],
        update_modifiers: vec![
            AccelModifierData {
                accel: Vec3::new(0.0, 1.2, 0.0),
            }
            .into(),
            UpdateModifierData::LinearDrag(LinearDragData { drag: 0.8 }),
        ],
        render_modifiers: vec![
            RenderModifierData::ParticleTexture {
                path: Some("textures/particles/smoke_puff.png".into()),
                sample_mapping: ParticleSampleMapping::Modulate,
            },
            RenderModifierData::ColorOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(0.5, 0.5, 0.5, 0.0),
                    },
                    GradientKeyData {
                        ratio: 0.1,
                        value: Vec4::new(0.45, 0.45, 0.45, 0.4),
                    },
                    GradientKeyData {
                        ratio: 0.6,
                        value: Vec4::new(0.35, 0.35, 0.35, 0.25),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.25, 0.25, 0.25, 0.0),
                    },
                ],
            },
            RenderModifierData::SizeOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(0.2, 0.2, 0.2, 0.0),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.8, 0.8, 0.8, 0.0),
                    },
                ],
            },
        ],
    }
}

fn sparks() -> ParticleEffectMarker {
    ParticleEffectMarker {
        capacity: 512,
        spawner: SpawnerConfig::Burst {
            count: 30.0,
            period: 0.5,
        },
        simulation_space: ParticleSimSpace::Global,
        simulation_condition: ParticleSimCondition::Always,
        motion_integration: ParticleMotionIntegration::PostUpdate,
        alpha_mode: ParticleAlphaMode::Add,
        init_modifiers: vec![
            InitModifierData::SetLifetime(ScalarRange::Random(0.3, 0.8)),
            InitModifierData::SetPositionSphere {
                center: Vec3::ZERO,
                radius: ScalarRange::Constant(0.05),
                volume: false,
            },
            InitModifierData::SetVelocitySphere {
                center: Vec3::ZERO,
                speed: ScalarRange::Random(3.0, 8.0),
            },
        ],
        update_modifiers: vec![AccelModifierData {
            accel: Vec3::new(0.0, -9.8, 0.0),
        }
        .into()],
        render_modifiers: vec![
            RenderModifierData::ParticleTexture {
                path: Some("textures/particles/spark.png".into()),
                sample_mapping: ParticleSampleMapping::Modulate,
            },
            RenderModifierData::ColorOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(1.0, 1.0, 0.8, 1.0),
                    },
                    GradientKeyData {
                        ratio: 0.5,
                        value: Vec4::new(1.0, 0.7, 0.2, 0.8),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.8, 0.2, 0.0, 0.0),
                    },
                ],
            },
            RenderModifierData::SizeOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(0.04, 0.04, 0.04, 0.0),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.01, 0.01, 0.01, 0.0),
                    },
                ],
            },
        ],
    }
}

fn fountain() -> ParticleEffectMarker {
    ParticleEffectMarker {
        capacity: 1024,
        spawner: SpawnerConfig::Rate { rate: 100.0 },
        simulation_space: ParticleSimSpace::Global,
        simulation_condition: ParticleSimCondition::Always,
        motion_integration: ParticleMotionIntegration::PostUpdate,
        alpha_mode: ParticleAlphaMode::Blend,
        init_modifiers: vec![
            InitModifierData::SetLifetime(ScalarRange::Random(1.5, 3.0)),
            InitModifierData::SetPositionCircle {
                center: Vec3::ZERO,
                axis: Vec3::Y,
                radius: ScalarRange::Random(0.05, 0.15),
                volume: false,
            },
            InitModifierData::SetVelocitySphere {
                center: Vec3::new(0.0, 6.0, 0.0),
                speed: ScalarRange::Random(0.5, 1.5),
            },
        ],
        update_modifiers: vec![AccelModifierData {
            accel: Vec3::new(0.0, -9.8, 0.0),
        }
        .into()],
        render_modifiers: vec![
            RenderModifierData::ParticleTexture {
                path: Some("textures/particles/droplet.png".into()),
                sample_mapping: ParticleSampleMapping::Modulate,
            },
            RenderModifierData::ColorOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(0.6, 0.85, 1.0, 0.9),
                    },
                    GradientKeyData {
                        ratio: 0.5,
                        value: Vec4::new(0.3, 0.6, 1.0, 0.7),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.1, 0.3, 0.8, 0.0),
                    },
                ],
            },
            RenderModifierData::SizeOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(0.08, 0.08, 0.08, 0.0),
                    },
                    GradientKeyData {
                        ratio: 0.5,
                        value: Vec4::new(0.06, 0.06, 0.06, 0.0),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.02, 0.02, 0.02, 0.0),
                    },
                ],
            },
        ],
    }
}

fn magic_orb() -> ParticleEffectMarker {
    ParticleEffectMarker {
        capacity: 512,
        spawner: SpawnerConfig::Rate { rate: 60.0 },
        simulation_space: ParticleSimSpace::Local,
        simulation_condition: ParticleSimCondition::Always,
        motion_integration: ParticleMotionIntegration::PostUpdate,
        alpha_mode: ParticleAlphaMode::Add,
        init_modifiers: vec![
            InitModifierData::SetLifetime(ScalarRange::Random(1.0, 2.5)),
            InitModifierData::SetPositionSphere {
                center: Vec3::ZERO,
                radius: ScalarRange::Random(0.3, 0.6),
                volume: false,
            },
            InitModifierData::SetVelocityTangent {
                origin: Vec3::ZERO,
                axis: Vec3::Y,
                speed: ScalarRange::Random(1.0, 2.0),
            },
        ],
        update_modifiers: vec![
            UpdateModifierData::RadialAccel(RadialAccelData {
                origin: Vec3::ZERO,
                accel: -2.0,
            }),
            UpdateModifierData::TangentAccel(TangentAccelData {
                origin: Vec3::ZERO,
                axis: Vec3::Y,
                accel: 3.0,
            }),
        ],
        render_modifiers: vec![
            RenderModifierData::ParticleTexture {
                path: Some("textures/particles/soft_circle.png".into()),
                sample_mapping: ParticleSampleMapping::Modulate,
            },
            RenderModifierData::ColorOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(0.8, 0.4, 1.0, 1.0),
                    },
                    GradientKeyData {
                        ratio: 0.5,
                        value: Vec4::new(0.4, 0.2, 1.0, 0.7),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.2, 0.05, 0.6, 0.0),
                    },
                ],
            },
            RenderModifierData::SizeOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(0.08, 0.08, 0.08, 0.0),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.02, 0.02, 0.02, 0.0),
                    },
                ],
            },
        ],
    }
}

fn snow() -> ParticleEffectMarker {
    ParticleEffectMarker {
        capacity: 1024,
        spawner: SpawnerConfig::Rate { rate: 40.0 },
        simulation_space: ParticleSimSpace::Global,
        simulation_condition: ParticleSimCondition::Always,
        motion_integration: ParticleMotionIntegration::PostUpdate,
        alpha_mode: ParticleAlphaMode::Blend,
        init_modifiers: vec![
            InitModifierData::SetLifetime(ScalarRange::Random(4.0, 8.0)),
            InitModifierData::SetPositionSphere {
                center: Vec3::new(0.0, 5.0, 0.0),
                radius: ScalarRange::Random(2.0, 5.0),
                volume: true,
            },
            InitModifierData::SetVelocitySphere {
                center: Vec3::ZERO,
                speed: ScalarRange::Random(0.1, 0.3),
            },
        ],
        update_modifiers: vec![
            AccelModifierData {
                accel: Vec3::new(0.0, -0.8, 0.0),
            }
            .into(),
            UpdateModifierData::LinearDrag(LinearDragData { drag: 2.0 }),
        ],
        render_modifiers: vec![
            RenderModifierData::ParticleTexture {
                path: Some("textures/particles/snowflake.png".into()),
                sample_mapping: ParticleSampleMapping::Modulate,
            },
            RenderModifierData::ColorOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(1.0, 1.0, 1.0, 0.0),
                    },
                    GradientKeyData {
                        ratio: 0.1,
                        value: Vec4::new(1.0, 1.0, 1.0, 0.8),
                    },
                    GradientKeyData {
                        ratio: 0.8,
                        value: Vec4::new(0.9, 0.95, 1.0, 0.6),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.8, 0.9, 1.0, 0.0),
                    },
                ],
            },
            RenderModifierData::SizeOverLifetime {
                keys: vec![
                    GradientKeyData {
                        ratio: 0.0,
                        value: Vec4::new(0.04, 0.04, 0.04, 0.0),
                    },
                    GradientKeyData {
                        ratio: 1.0,
                        value: Vec4::new(0.03, 0.03, 0.03, 0.0),
                    },
                ],
            },
        ],
    }
}
