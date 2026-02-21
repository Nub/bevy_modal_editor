//! Built-in VFX presets matching the original particle effect library.

use bevy::prelude::*;

use crate::curve::{Curve, CurveKey, Gradient, GradientKey, Interp};
use crate::data::*;

/// Return the built-in default VFX presets as `(name, system)` pairs.
pub fn default_presets() -> Vec<(&'static str, VfxSystem)> {
    vec![
        ("Fire", fire()),
        ("Smoke", smoke()),
        ("Sparks", sparks()),
        ("Fountain", fountain()),
        ("Magic Orb", magic_orb()),
        ("Snow", snow()),
        ("Fireflies", fireflies()),
        ("Explosion", explosion()),
        ("Rain", rain()),
        ("Portal", portal()),
        ("Embers", embers()),
        ("Dust Motes", dust_motes()),
        ("Flamethrower", flamethrower()),
        ("Heal Aura", heal_aura()),
        ("Waterfall", waterfall()),
        ("Campfire", campfire()),
        ("Rock Debris", rock_debris()),
    ]
}

/// Multi-layer fire: hot core + flame billows + rising tips + embers.
fn fire() -> VfxSystem {
    VfxSystem {
        emitters: vec![
            // Hot core — bright additive glow at the base
            EmitterDef {
                name: "Hot Core".to_string(),
                enabled: true,
                capacity: 128,
                spawn: SpawnModule::Rate(25.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(0.2, 0.5)),
                    InitModule::SetPosition(ShapeEmitter::Circle {
                        center: Vec3::ZERO,
                        axis: Vec3::Y,
                        radius: ScalarRange::Random(0.02, 0.1),
                    }),
                    InitModule::SetVelocity(VelocityMode::Cone {
                        direction: Vec3::Y,
                        angle: 0.15,
                        speed: ScalarRange::Random(0.5, 1.2),
                    }),
                ],
                update: vec![
                    UpdateModule::Drag(2.0),
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(3.0, 2.5, 1.0, 1.0) },
                            GradientKey { time: 0.4, color: LinearRgba::new(2.0, 1.2, 0.3, 0.8) },
                            GradientKey { time: 1.0, color: LinearRgba::new(1.0, 0.4, 0.0, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.15, interp: Interp::EaseOut },
                            CurveKey { time: 0.5, value: 0.25, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 0.1, interp: Interp::EaseIn },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/fire_01.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
            // Flame body — main visible fire shapes
            EmitterDef {
                name: "Flame Body".to_string(),
                enabled: true,
                capacity: 256,
                spawn: SpawnModule::Rate(50.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(0.3, 0.8)),
                    InitModule::SetPosition(ShapeEmitter::Circle {
                        center: Vec3::ZERO,
                        axis: Vec3::Y,
                        radius: ScalarRange::Random(0.05, 0.2),
                    }),
                    InitModule::SetVelocity(VelocityMode::Cone {
                        direction: Vec3::Y,
                        angle: 0.25,
                        speed: ScalarRange::Random(1.0, 2.5),
                    }),
                ],
                update: vec![
                    UpdateModule::Gravity(Vec3::new(0.0, 2.0, 0.0)),
                    UpdateModule::Drag(2.5),
                    UpdateModule::Noise {
                        strength: 1.2,
                        frequency: 3.0,
                        scroll: Vec3::new(0.0, 2.5, 0.0),
                    },
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(1.5, 1.0, 0.3, 1.0) },
                            GradientKey { time: 0.25, color: LinearRgba::new(1.2, 0.6, 0.1, 0.9) },
                            GradientKey { time: 0.6, color: LinearRgba::new(0.8, 0.2, 0.0, 0.5) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.3, 0.05, 0.0, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.12, interp: Interp::Linear },
                            CurveKey { time: 0.3, value: 0.22, interp: Interp::EaseOut },
                            CurveKey { time: 1.0, value: 0.06, interp: Interp::EaseIn },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/flame_01.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
            // Flame tips — tall thin tongues that lick upward
            EmitterDef {
                name: "Flame Tips".to_string(),
                enabled: true,
                capacity: 128,
                spawn: SpawnModule::Rate(20.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(0.4, 0.9)),
                    InitModule::SetPosition(ShapeEmitter::Circle {
                        center: Vec3::new(0.0, 0.1, 0.0),
                        axis: Vec3::Y,
                        radius: ScalarRange::Random(0.03, 0.15),
                    }),
                    InitModule::SetVelocity(VelocityMode::Cone {
                        direction: Vec3::Y,
                        angle: 0.2,
                        speed: ScalarRange::Random(1.5, 3.0),
                    }),
                ],
                update: vec![
                    UpdateModule::Gravity(Vec3::new(0.0, 1.5, 0.0)),
                    UpdateModule::Drag(2.0),
                    UpdateModule::Noise {
                        strength: 0.8,
                        frequency: 2.5,
                        scroll: Vec3::new(0.3, 3.0, 0.3),
                    },
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(1.5, 0.8, 0.15, 0.9) },
                            GradientKey { time: 0.4, color: LinearRgba::new(1.0, 0.35, 0.02, 0.6) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.4, 0.08, 0.0, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.1, interp: Interp::Linear },
                            CurveKey { time: 0.4, value: 0.18, interp: Interp::EaseOut },
                            CurveKey { time: 1.0, value: 0.04, interp: Interp::EaseIn },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/flame_05.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
        ],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn smoke() -> VfxSystem {
    VfxSystem {
        emitters: vec![EmitterDef {
            name: "Smoke Puff".to_string(),
            enabled: true,
            capacity: 256,
            spawn: SpawnModule::Rate(20.0),
            init: vec![
                InitModule::SetLifetime(ScalarRange::Random(2.0, 4.0)),
                InitModule::SetPosition(ShapeEmitter::Sphere {
                    center: Vec3::ZERO,
                    radius: ScalarRange::Random(0.1, 0.4),
                }),
                InitModule::SetVelocity(VelocityMode::Radial {
                    center: Vec3::ZERO,
                    speed: ScalarRange::Random(0.3, 0.8),
                }),
            ],
            update: vec![
                UpdateModule::Gravity(Vec3::new(0.0, 1.2, 0.0)),
                UpdateModule::Drag(0.8),
                UpdateModule::ColorByLife(Gradient {
                    keys: vec![
                        GradientKey {
                            time: 0.0,
                            color: LinearRgba::new(0.5, 0.5, 0.5, 0.0),
                        },
                        GradientKey {
                            time: 0.1,
                            color: LinearRgba::new(0.45, 0.45, 0.45, 0.4),
                        },
                        GradientKey {
                            time: 0.6,
                            color: LinearRgba::new(0.35, 0.35, 0.35, 0.25),
                        },
                        GradientKey {
                            time: 1.0,
                            color: LinearRgba::new(0.25, 0.25, 0.25, 0.0),
                        },
                    ],
                }),
                UpdateModule::SizeByLife(Curve {
                    keys: vec![
                        CurveKey { time: 0.0, value: 0.2, interp: Interp::Linear },
                        CurveKey { time: 1.0, value: 0.8, interp: Interp::Linear },
                    ],
                }),
            ],
            render: RenderModule::Billboard(BillboardConfig {
                texture: Some("textures/particles/smoke_01.png".into()),
                ..default()
            }),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Blend,
        }],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn sparks() -> VfxSystem {
    VfxSystem {
        emitters: vec![EmitterDef {
            name: "Sparks".to_string(),
            enabled: true,
            capacity: 512,
            spawn: SpawnModule::Burst {
                count: 30,
                interval: 0.5,
                max_cycles: None,
                offset: 0.0,
            },
            init: vec![
                InitModule::SetLifetime(ScalarRange::Random(0.3, 0.8)),
                InitModule::SetPosition(ShapeEmitter::Sphere {
                    center: Vec3::ZERO,
                    radius: ScalarRange::Constant(0.05),
                }),
                InitModule::SetVelocity(VelocityMode::Radial {
                    center: Vec3::ZERO,
                    speed: ScalarRange::Random(3.0, 8.0),
                }),
            ],
            update: vec![
                UpdateModule::Gravity(Vec3::new(0.0, -9.8, 0.0)),
                UpdateModule::ColorByLife(Gradient {
                    keys: vec![
                        GradientKey {
                            time: 0.0,
                            color: LinearRgba::new(1.0, 1.0, 0.8, 1.0),
                        },
                        GradientKey {
                            time: 0.5,
                            color: LinearRgba::new(1.0, 0.7, 0.2, 0.8),
                        },
                        GradientKey {
                            time: 1.0,
                            color: LinearRgba::new(0.8, 0.2, 0.0, 0.0),
                        },
                    ],
                }),
                UpdateModule::SizeByLife(Curve {
                    keys: vec![
                        CurveKey { time: 0.0, value: 0.04, interp: Interp::Linear },
                        CurveKey { time: 1.0, value: 0.01, interp: Interp::Linear },
                    ],
                }),
            ],
            render: RenderModule::Billboard(BillboardConfig {
                texture: Some("textures/particles/spark_02.png".into()),
                ..default()
            }),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Additive,
        }],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn fountain() -> VfxSystem {
    VfxSystem {
        emitters: vec![EmitterDef {
            name: "Water".to_string(),
            enabled: true,
            capacity: 1024,
            spawn: SpawnModule::Rate(100.0),
            init: vec![
                InitModule::SetLifetime(ScalarRange::Random(1.5, 3.0)),
                InitModule::SetPosition(ShapeEmitter::Circle {
                    center: Vec3::ZERO,
                    axis: Vec3::Y,
                    radius: ScalarRange::Random(0.05, 0.15),
                }),
                InitModule::SetVelocity(VelocityMode::Radial {
                    center: Vec3::new(0.0, 6.0, 0.0),
                    speed: ScalarRange::Random(0.5, 1.5),
                }),
            ],
            update: vec![
                UpdateModule::Gravity(Vec3::new(0.0, -9.8, 0.0)),
                UpdateModule::ColorByLife(Gradient {
                    keys: vec![
                        GradientKey {
                            time: 0.0,
                            color: LinearRgba::new(0.6, 0.85, 1.0, 0.9),
                        },
                        GradientKey {
                            time: 0.5,
                            color: LinearRgba::new(0.3, 0.6, 1.0, 0.7),
                        },
                        GradientKey {
                            time: 1.0,
                            color: LinearRgba::new(0.1, 0.3, 0.8, 0.0),
                        },
                    ],
                }),
                UpdateModule::SizeByLife(Curve {
                    keys: vec![
                        CurveKey { time: 0.0, value: 0.08, interp: Interp::Linear },
                        CurveKey { time: 0.5, value: 0.06, interp: Interp::Linear },
                        CurveKey { time: 1.0, value: 0.02, interp: Interp::Linear },
                    ],
                }),
            ],
            render: RenderModule::Billboard(BillboardConfig {
                texture: Some("textures/particles/circle_01.png".into()),
                ..default()
            }),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Blend,
        }],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn magic_orb() -> VfxSystem {
    VfxSystem {
        emitters: vec![EmitterDef {
            name: "Orb Particles".to_string(),
            enabled: true,
            capacity: 512,
            spawn: SpawnModule::Rate(60.0),
            init: vec![
                InitModule::SetLifetime(ScalarRange::Random(1.0, 2.5)),
                InitModule::SetPosition(ShapeEmitter::Sphere {
                    center: Vec3::ZERO,
                    radius: ScalarRange::Random(0.3, 0.6),
                }),
                InitModule::SetVelocity(VelocityMode::Tangent {
                    axis: Vec3::Y,
                    speed: ScalarRange::Random(1.0, 2.0),
                }),
            ],
            update: vec![
                UpdateModule::RadialAccel {
                    origin: Vec3::ZERO,
                    accel: -2.0,
                },
                UpdateModule::TangentAccel {
                    origin: Vec3::ZERO,
                    axis: Vec3::Y,
                    accel: 3.0,
                },
                UpdateModule::ColorByLife(Gradient {
                    keys: vec![
                        GradientKey {
                            time: 0.0,
                            color: LinearRgba::new(0.8, 0.4, 1.0, 1.0),
                        },
                        GradientKey {
                            time: 0.5,
                            color: LinearRgba::new(0.4, 0.2, 1.0, 0.7),
                        },
                        GradientKey {
                            time: 1.0,
                            color: LinearRgba::new(0.2, 0.05, 0.6, 0.0),
                        },
                    ],
                }),
                UpdateModule::SizeByLife(Curve {
                    keys: vec![
                        CurveKey { time: 0.0, value: 0.08, interp: Interp::Linear },
                        CurveKey { time: 1.0, value: 0.02, interp: Interp::Linear },
                    ],
                }),
            ],
            render: RenderModule::Billboard(BillboardConfig {
                texture: Some("textures/particles/magic_01.png".into()),
                ..default()
            }),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Additive,
        }],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn fireflies() -> VfxSystem {
    VfxSystem {
        emitters: vec![EmitterDef {
            name: "Fireflies".to_string(),
            enabled: true,
            capacity: 128,
            spawn: SpawnModule::Rate(8.0),
            init: vec![
                InitModule::SetLifetime(ScalarRange::Random(3.0, 6.0)),
                InitModule::SetPosition(ShapeEmitter::Box {
                    center: Vec3::ZERO,
                    half_extents: Vec3::new(3.0, 1.5, 3.0),
                }),
                InitModule::SetVelocity(VelocityMode::Random {
                    speed: ScalarRange::Random(0.1, 0.4),
                }),
            ],
            update: vec![
                UpdateModule::Noise {
                    strength: 1.5,
                    frequency: 0.8,
                    scroll: Vec3::new(0.3, 0.5, 0.2),
                },
                UpdateModule::Drag(0.5),
                UpdateModule::ColorByLife(Gradient {
                    keys: vec![
                        GradientKey { time: 0.0, color: LinearRgba::new(0.5, 1.0, 0.3, 0.0) },
                        GradientKey { time: 0.15, color: LinearRgba::new(0.7, 1.0, 0.2, 1.0) },
                        GradientKey { time: 0.5, color: LinearRgba::new(0.4, 0.8, 0.1, 0.6) },
                        GradientKey { time: 0.85, color: LinearRgba::new(0.6, 1.0, 0.3, 0.8) },
                        GradientKey { time: 1.0, color: LinearRgba::new(0.3, 0.6, 0.1, 0.0) },
                    ],
                }),
                UpdateModule::SizeByLife(Curve {
                    keys: vec![
                        CurveKey { time: 0.0, value: 0.03, interp: Interp::EaseIn },
                        CurveKey { time: 0.5, value: 0.06, interp: Interp::EaseOut },
                        CurveKey { time: 1.0, value: 0.02, interp: Interp::Linear },
                    ],
                }),
            ],
            render: RenderModule::Billboard(BillboardConfig {
                texture: Some("textures/particles/light_01.png".into()),
                ..default()
            }),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Additive,
        }],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn explosion() -> VfxSystem {
    VfxSystem {
        emitters: vec![
            // Core flash
            EmitterDef {
                name: "Flash".to_string(),
                enabled: true,
                capacity: 8,
                spawn: SpawnModule::Once { offset: 0.0, count: 4 },
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(0.1, 0.25)),
                    InitModule::SetPosition(ShapeEmitter::Point(Vec3::ZERO)),
                    InitModule::SetVelocity(VelocityMode::Radial {
                        center: Vec3::ZERO,
                        speed: ScalarRange::Random(0.5, 1.0),
                    }),
                    InitModule::SetSize(ScalarRange::Random(0.8, 1.5)),
                ],
                update: vec![
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(4.0, 3.5, 2.0, 1.0) },
                            GradientKey { time: 0.5, color: LinearRgba::new(2.0, 1.0, 0.3, 0.5) },
                            GradientKey { time: 1.0, color: LinearRgba::new(1.0, 0.3, 0.0, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.5, interp: Interp::EaseOut },
                            CurveKey { time: 1.0, value: 2.0, interp: Interp::Linear },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/light_02.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
            // Fireball
            EmitterDef {
                name: "Fireball".to_string(),
                enabled: true,
                capacity: 64,
                spawn: SpawnModule::Once { offset: 0.0, count: 20 },
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(0.3, 0.8)),
                    InitModule::SetPosition(ShapeEmitter::Sphere {
                        center: Vec3::ZERO,
                        radius: ScalarRange::Random(0.1, 0.3),
                    }),
                    InitModule::SetVelocity(VelocityMode::Radial {
                        center: Vec3::ZERO,
                        speed: ScalarRange::Random(2.0, 5.0),
                    }),
                ],
                update: vec![
                    UpdateModule::Drag(3.0),
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(2.0, 1.5, 0.5, 1.0) },
                            GradientKey { time: 0.3, color: LinearRgba::new(1.5, 0.6, 0.1, 0.8) },
                            GradientKey { time: 0.7, color: LinearRgba::new(0.6, 0.1, 0.0, 0.3) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.2, 0.02, 0.0, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.2, interp: Interp::EaseOut },
                            CurveKey { time: 0.5, value: 0.5, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 0.8, interp: Interp::Linear },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/fire_02.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
            // Debris sparks
            EmitterDef {
                name: "Debris".to_string(),
                enabled: true,
                capacity: 256,
                spawn: SpawnModule::Once { offset: 0.0, count: 80 },
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(0.5, 1.5)),
                    InitModule::SetPosition(ShapeEmitter::Sphere {
                        center: Vec3::ZERO,
                        radius: ScalarRange::Random(0.05, 0.2),
                    }),
                    InitModule::SetVelocity(VelocityMode::Radial {
                        center: Vec3::ZERO,
                        speed: ScalarRange::Random(4.0, 12.0),
                    }),
                ],
                update: vec![
                    UpdateModule::Gravity(Vec3::new(0.0, -9.8, 0.0)),
                    UpdateModule::Drag(0.5),
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(1.0, 0.8, 0.3, 1.0) },
                            GradientKey { time: 0.3, color: LinearRgba::new(1.0, 0.4, 0.1, 0.9) },
                            GradientKey { time: 0.7, color: LinearRgba::new(0.5, 0.15, 0.0, 0.5) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.2, 0.05, 0.0, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.06, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 0.01, interp: Interp::Linear },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/spark_03.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
            // Smoke cloud
            EmitterDef {
                name: "Smoke".to_string(),
                enabled: true,
                capacity: 64,
                spawn: SpawnModule::Once { offset: 0.0, count: 20 },
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(1.0, 3.0)),
                    InitModule::SetPosition(ShapeEmitter::Sphere {
                        center: Vec3::ZERO,
                        radius: ScalarRange::Random(0.2, 0.5),
                    }),
                    InitModule::SetVelocity(VelocityMode::Radial {
                        center: Vec3::ZERO,
                        speed: ScalarRange::Random(1.0, 3.0),
                    }),
                ],
                update: vec![
                    UpdateModule::Gravity(Vec3::new(0.0, 1.5, 0.0)),
                    UpdateModule::Drag(2.0),
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(0.4, 0.35, 0.3, 0.0) },
                            GradientKey { time: 0.15, color: LinearRgba::new(0.35, 0.3, 0.25, 0.5) },
                            GradientKey { time: 0.6, color: LinearRgba::new(0.25, 0.22, 0.2, 0.3) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.2, 0.18, 0.15, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.3, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 1.5, interp: Interp::EaseOut },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/smoke_04.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Blend,
            },
        ],
        params: Vec::new(),
        duration: 3.0,
        looping: false,
    }
}

fn rain() -> VfxSystem {
    VfxSystem {
        emitters: vec![EmitterDef {
            name: "Raindrops".to_string(),
            enabled: true,
            capacity: 2048,
            spawn: SpawnModule::Rate(200.0),
            init: vec![
                InitModule::SetLifetime(ScalarRange::Random(0.8, 1.5)),
                InitModule::SetPosition(ShapeEmitter::Box {
                    center: Vec3::new(0.0, 8.0, 0.0),
                    half_extents: Vec3::new(6.0, 0.5, 6.0),
                }),
                InitModule::SetVelocity(VelocityMode::Directional {
                    direction: Vec3::new(-0.1, -1.0, 0.0).normalize(),
                    speed: ScalarRange::Random(8.0, 14.0),
                }),
                InitModule::SetSize(ScalarRange::Random(0.02, 0.04)),
            ],
            update: vec![
                UpdateModule::ColorByLife(Gradient {
                    keys: vec![
                        GradientKey { time: 0.0, color: LinearRgba::new(0.6, 0.7, 0.85, 0.0) },
                        GradientKey { time: 0.05, color: LinearRgba::new(0.6, 0.7, 0.85, 0.5) },
                        GradientKey { time: 0.9, color: LinearRgba::new(0.5, 0.6, 0.8, 0.4) },
                        GradientKey { time: 1.0, color: LinearRgba::new(0.4, 0.5, 0.7, 0.0) },
                    ],
                }),
                UpdateModule::SizeBySpeed {
                    min_speed: 0.0,
                    max_speed: 15.0,
                    min_size: 0.02,
                    max_size: 0.08,
                },
                UpdateModule::KillZone {
                    shape: KillShape::Box {
                        center: Vec3::new(0.0, -1.0, 0.0),
                        half_extents: Vec3::new(100.0, 0.5, 100.0),
                    },
                    invert: false,
                },
            ],
            render: RenderModule::Billboard(BillboardConfig {
                orient: BillboardOrient::AlongVelocity,
                texture: Some("textures/particles/circle_01.png".into()),
                ..default()
            }),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Blend,
        }],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn portal() -> VfxSystem {
    VfxSystem {
        emitters: vec![
            // Ring particles
            EmitterDef {
                name: "Ring".to_string(),
                enabled: true,
                capacity: 512,
                spawn: SpawnModule::Rate(60.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(1.5, 3.0)),
                    InitModule::SetPosition(ShapeEmitter::Circle {
                        center: Vec3::ZERO,
                        axis: Vec3::Y,
                        radius: ScalarRange::Random(0.8, 1.2),
                    }),
                    InitModule::SetVelocity(VelocityMode::Tangent {
                        axis: Vec3::Y,
                        speed: ScalarRange::Random(1.5, 3.0),
                    }),
                ],
                update: vec![
                    UpdateModule::RadialAccel {
                        origin: Vec3::ZERO,
                        accel: -1.5,
                    },
                    UpdateModule::Drag(0.3),
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(0.2, 0.6, 1.0, 0.0) },
                            GradientKey { time: 0.1, color: LinearRgba::new(0.3, 0.7, 1.0, 1.0) },
                            GradientKey { time: 0.5, color: LinearRgba::new(0.5, 0.3, 1.0, 0.8) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.8, 0.2, 1.0, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.06, interp: Interp::EaseIn },
                            CurveKey { time: 0.3, value: 0.1, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 0.02, interp: Interp::EaseOut },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/magic_03.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
            // Center glow
            EmitterDef {
                name: "Core Glow".to_string(),
                enabled: true,
                capacity: 32,
                spawn: SpawnModule::Rate(6.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(0.5, 1.0)),
                    InitModule::SetPosition(ShapeEmitter::Point(Vec3::ZERO)),
                    InitModule::SetVelocity(VelocityMode::Random {
                        speed: ScalarRange::Random(0.05, 0.1),
                    }),
                    InitModule::SetSize(ScalarRange::Random(0.5, 0.9)),
                ],
                update: vec![
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(0.6, 0.4, 1.0, 0.0) },
                            GradientKey { time: 0.3, color: LinearRgba::new(0.4, 0.5, 1.0, 0.3) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.3, 0.2, 0.8, 0.0) },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/light_01.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
        ],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn embers() -> VfxSystem {
    VfxSystem {
        emitters: vec![EmitterDef {
            name: "Embers".to_string(),
            enabled: true,
            capacity: 256,
            spawn: SpawnModule::Rate(15.0),
            init: vec![
                InitModule::SetLifetime(ScalarRange::Random(2.0, 5.0)),
                InitModule::SetPosition(ShapeEmitter::Box {
                    center: Vec3::ZERO,
                    half_extents: Vec3::new(0.5, 0.1, 0.5),
                }),
                InitModule::SetVelocity(VelocityMode::Directional {
                    direction: Vec3::Y,
                    speed: ScalarRange::Random(0.3, 1.0),
                }),
            ],
            update: vec![
                UpdateModule::Gravity(Vec3::new(0.0, 0.4, 0.0)),
                UpdateModule::Noise {
                    strength: 0.8,
                    frequency: 0.5,
                    scroll: Vec3::new(0.2, 0.3, 0.1),
                },
                UpdateModule::Drag(0.3),
                UpdateModule::ColorByLife(Gradient {
                    keys: vec![
                        GradientKey { time: 0.0, color: LinearRgba::new(1.0, 0.6, 0.1, 1.0) },
                        GradientKey { time: 0.3, color: LinearRgba::new(1.0, 0.3, 0.0, 0.8) },
                        GradientKey { time: 0.7, color: LinearRgba::new(0.6, 0.1, 0.0, 0.4) },
                        GradientKey { time: 1.0, color: LinearRgba::new(0.2, 0.02, 0.0, 0.0) },
                    ],
                }),
                UpdateModule::SizeByLife(Curve {
                    keys: vec![
                        CurveKey { time: 0.0, value: 0.04, interp: Interp::Linear },
                        CurveKey { time: 0.5, value: 0.03, interp: Interp::Linear },
                        CurveKey { time: 1.0, value: 0.01, interp: Interp::Linear },
                    ],
                }),
            ],
            render: RenderModule::Billboard(BillboardConfig {
                texture: Some("textures/particles/spark_04.png".into()),
                ..default()
            }),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Additive,
        }],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn dust_motes() -> VfxSystem {
    VfxSystem {
        emitters: vec![EmitterDef {
            name: "Dust".to_string(),
            enabled: true,
            capacity: 128,
            spawn: SpawnModule::Rate(5.0),
            init: vec![
                InitModule::SetLifetime(ScalarRange::Random(5.0, 10.0)),
                InitModule::SetPosition(ShapeEmitter::Box {
                    center: Vec3::ZERO,
                    half_extents: Vec3::new(3.0, 2.0, 3.0),
                }),
                InitModule::SetVelocity(VelocityMode::Random {
                    speed: ScalarRange::Random(0.02, 0.08),
                }),
                InitModule::SetSize(ScalarRange::Random(0.01, 0.03)),
            ],
            update: vec![
                UpdateModule::Noise {
                    strength: 0.3,
                    frequency: 0.3,
                    scroll: Vec3::new(0.1, 0.05, 0.1),
                },
                UpdateModule::ColorByLife(Gradient {
                    keys: vec![
                        GradientKey { time: 0.0, color: LinearRgba::new(1.0, 0.95, 0.8, 0.0) },
                        GradientKey { time: 0.15, color: LinearRgba::new(1.0, 0.95, 0.8, 0.3) },
                        GradientKey { time: 0.85, color: LinearRgba::new(0.9, 0.85, 0.7, 0.2) },
                        GradientKey { time: 1.0, color: LinearRgba::new(0.8, 0.75, 0.6, 0.0) },
                    ],
                }),
            ],
            render: RenderModule::Billboard(BillboardConfig {
                texture: Some("textures/particles/circle_05.png".into()),
                ..default()
            }),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Blend,
        }],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn flamethrower() -> VfxSystem {
    VfxSystem {
        emitters: vec![EmitterDef {
            name: "Flame Jet".to_string(),
            enabled: true,
            capacity: 1024,
            spawn: SpawnModule::Rate(150.0),
            init: vec![
                InitModule::SetLifetime(ScalarRange::Random(0.3, 0.7)),
                InitModule::SetPosition(ShapeEmitter::Sphere {
                    center: Vec3::ZERO,
                    radius: ScalarRange::Random(0.02, 0.08),
                }),
                InitModule::SetVelocity(VelocityMode::Cone {
                    direction: Vec3::Z,
                    angle: 0.2,
                    speed: ScalarRange::Random(6.0, 10.0),
                }),
            ],
            update: vec![
                UpdateModule::Gravity(Vec3::new(0.0, 2.0, 0.0)),
                UpdateModule::Drag(1.0),
                UpdateModule::ColorByLife(Gradient {
                    keys: vec![
                        GradientKey { time: 0.0, color: LinearRgba::new(2.0, 1.8, 1.0, 1.0) },
                        GradientKey { time: 0.2, color: LinearRgba::new(1.5, 0.8, 0.2, 0.9) },
                        GradientKey { time: 0.5, color: LinearRgba::new(1.0, 0.3, 0.0, 0.6) },
                        GradientKey { time: 0.8, color: LinearRgba::new(0.4, 0.1, 0.0, 0.3) },
                        GradientKey { time: 1.0, color: LinearRgba::new(0.15, 0.05, 0.0, 0.0) },
                    ],
                }),
                UpdateModule::SizeByLife(Curve {
                    keys: vec![
                        CurveKey { time: 0.0, value: 0.05, interp: Interp::Linear },
                        CurveKey { time: 0.3, value: 0.2, interp: Interp::EaseOut },
                        CurveKey { time: 1.0, value: 0.4, interp: Interp::Linear },
                    ],
                }),
                UpdateModule::Noise {
                    strength: 2.0,
                    frequency: 2.0,
                    scroll: Vec3::new(0.0, 3.0, 0.0),
                },
            ],
            render: RenderModule::Billboard(BillboardConfig {
                texture: Some("textures/particles/flame_02.png".into()),
                ..default()
            }),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Additive,
        }],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn heal_aura() -> VfxSystem {
    VfxSystem {
        emitters: vec![
            // Rising sparkles
            EmitterDef {
                name: "Sparkles".to_string(),
                enabled: true,
                capacity: 256,
                spawn: SpawnModule::Rate(30.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(1.0, 2.0)),
                    InitModule::SetPosition(ShapeEmitter::Circle {
                        center: Vec3::ZERO,
                        axis: Vec3::Y,
                        radius: ScalarRange::Random(0.3, 0.8),
                    }),
                    InitModule::SetVelocity(VelocityMode::Directional {
                        direction: Vec3::Y,
                        speed: ScalarRange::Random(0.8, 1.5),
                    }),
                ],
                update: vec![
                    UpdateModule::Drag(1.0),
                    UpdateModule::OrbitAround {
                        axis: Vec3::Y,
                        speed: 2.0,
                        radius_decay: 0.3,
                    },
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(0.3, 1.0, 0.5, 0.0) },
                            GradientKey { time: 0.15, color: LinearRgba::new(0.4, 1.0, 0.6, 0.9) },
                            GradientKey { time: 0.7, color: LinearRgba::new(0.2, 0.8, 0.4, 0.5) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.1, 0.5, 0.2, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.04, interp: Interp::EaseIn },
                            CurveKey { time: 0.3, value: 0.07, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 0.02, interp: Interp::EaseOut },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/magic_01.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
            // Ground ring
            EmitterDef {
                name: "Ring Pulse".to_string(),
                enabled: true,
                capacity: 64,
                spawn: SpawnModule::Rate(8.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(0.8, 1.5)),
                    InitModule::SetPosition(ShapeEmitter::Circle {
                        center: Vec3::ZERO,
                        axis: Vec3::Y,
                        radius: ScalarRange::Random(0.6, 1.0),
                    }),
                    InitModule::SetVelocity(VelocityMode::Radial {
                        center: Vec3::ZERO,
                        speed: ScalarRange::Random(0.3, 0.6),
                    }),
                    InitModule::SetSize(ScalarRange::Random(0.1, 0.2)),
                ],
                update: vec![
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(0.2, 0.8, 0.3, 0.0) },
                            GradientKey { time: 0.2, color: LinearRgba::new(0.3, 1.0, 0.5, 0.4) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.1, 0.5, 0.2, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.1, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 0.3, interp: Interp::EaseOut },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/light_01.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
        ],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn waterfall() -> VfxSystem {
    VfxSystem {
        emitters: vec![
            // Main water stream
            EmitterDef {
                name: "Water Stream".to_string(),
                enabled: true,
                capacity: 2048,
                spawn: SpawnModule::Rate(180.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(1.0, 2.0)),
                    InitModule::SetPosition(ShapeEmitter::Edge {
                        start: Vec3::new(-1.0, 3.0, 0.0),
                        end: Vec3::new(1.0, 3.0, 0.0),
                    }),
                    InitModule::SetVelocity(VelocityMode::Directional {
                        direction: Vec3::new(0.0, -1.0, 0.5).normalize(),
                        speed: ScalarRange::Random(1.0, 2.5),
                    }),
                ],
                update: vec![
                    UpdateModule::Gravity(Vec3::new(0.0, -6.0, 0.0)),
                    UpdateModule::Noise {
                        strength: 0.5,
                        frequency: 1.5,
                        scroll: Vec3::new(0.0, -2.0, 0.0),
                    },
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(0.7, 0.85, 1.0, 0.8) },
                            GradientKey { time: 0.5, color: LinearRgba::new(0.5, 0.7, 0.95, 0.6) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.3, 0.5, 0.8, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.04, interp: Interp::Linear },
                            CurveKey { time: 0.3, value: 0.06, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 0.08, interp: Interp::EaseOut },
                        ],
                    }),
                    UpdateModule::KillZone {
                        shape: KillShape::Box {
                            center: Vec3::new(0.0, -1.0, 0.0),
                            half_extents: Vec3::new(100.0, 0.3, 100.0),
                        },
                        invert: false,
                    },
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/circle_01.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Blend,
            },
            // Mist at base
            EmitterDef {
                name: "Mist".to_string(),
                enabled: true,
                capacity: 128,
                spawn: SpawnModule::Rate(12.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(1.5, 3.0)),
                    InitModule::SetPosition(ShapeEmitter::Box {
                        center: Vec3::new(0.0, -0.5, 0.3),
                        half_extents: Vec3::new(1.2, 0.2, 0.5),
                    }),
                    InitModule::SetVelocity(VelocityMode::Directional {
                        direction: Vec3::new(0.0, 0.5, 1.0).normalize(),
                        speed: ScalarRange::Random(0.2, 0.6),
                    }),
                    InitModule::SetSize(ScalarRange::Random(0.3, 0.6)),
                ],
                update: vec![
                    UpdateModule::Drag(0.5),
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(0.8, 0.9, 1.0, 0.0) },
                            GradientKey { time: 0.2, color: LinearRgba::new(0.8, 0.9, 1.0, 0.2) },
                            GradientKey { time: 0.7, color: LinearRgba::new(0.7, 0.8, 0.9, 0.1) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.6, 0.7, 0.8, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.3, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 1.0, interp: Interp::EaseOut },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/smoke_01.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Blend,
            },
        ],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

/// Multi-layer campfire: hot core + flame body + flame wisps + embers + smoke.
fn campfire() -> VfxSystem {
    VfxSystem {
        emitters: vec![
            // Hot core — bright white-yellow at the heart of the fire
            EmitterDef {
                name: "Hot Core".to_string(),
                enabled: true,
                capacity: 64,
                spawn: SpawnModule::Rate(15.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(0.15, 0.35)),
                    InitModule::SetPosition(ShapeEmitter::Circle {
                        center: Vec3::ZERO,
                        axis: Vec3::Y,
                        radius: ScalarRange::Random(0.01, 0.08),
                    }),
                    InitModule::SetVelocity(VelocityMode::Cone {
                        direction: Vec3::Y,
                        angle: 0.1,
                        speed: ScalarRange::Random(0.3, 0.8),
                    }),
                ],
                update: vec![
                    UpdateModule::Drag(3.0),
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(4.0, 3.5, 2.0, 1.0) },
                            GradientKey { time: 0.5, color: LinearRgba::new(2.5, 1.5, 0.4, 0.8) },
                            GradientKey { time: 1.0, color: LinearRgba::new(1.5, 0.5, 0.0, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.12, interp: Interp::EaseOut },
                            CurveKey { time: 0.4, value: 0.18, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 0.06, interp: Interp::EaseIn },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/fire_01.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
            // Flame body — main billowy fire shapes
            EmitterDef {
                name: "Flames".to_string(),
                enabled: true,
                capacity: 256,
                spawn: SpawnModule::Rate(45.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(0.4, 0.9)),
                    InitModule::SetPosition(ShapeEmitter::Circle {
                        center: Vec3::ZERO,
                        axis: Vec3::Y,
                        radius: ScalarRange::Random(0.04, 0.18),
                    }),
                    InitModule::SetVelocity(VelocityMode::Cone {
                        direction: Vec3::Y,
                        angle: 0.3,
                        speed: ScalarRange::Random(1.0, 2.5),
                    }),
                ],
                update: vec![
                    UpdateModule::Gravity(Vec3::new(0.0, 1.5, 0.0)),
                    UpdateModule::Drag(2.0),
                    UpdateModule::Noise {
                        strength: 1.0,
                        frequency: 3.0,
                        scroll: Vec3::new(0.0, 2.0, 0.0),
                    },
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(1.5, 1.0, 0.3, 1.0) },
                            GradientKey { time: 0.2, color: LinearRgba::new(1.2, 0.6, 0.12, 0.9) },
                            GradientKey { time: 0.6, color: LinearRgba::new(0.8, 0.2, 0.0, 0.5) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.3, 0.05, 0.0, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.1, interp: Interp::Linear },
                            CurveKey { time: 0.3, value: 0.2, interp: Interp::EaseOut },
                            CurveKey { time: 1.0, value: 0.05, interp: Interp::EaseIn },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/flame_03.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
            // Flame wisps — thin licking flame tongues
            EmitterDef {
                name: "Flame Wisps".to_string(),
                enabled: true,
                capacity: 128,
                spawn: SpawnModule::Rate(18.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(0.3, 0.7)),
                    InitModule::SetPosition(ShapeEmitter::Circle {
                        center: Vec3::new(0.0, 0.05, 0.0),
                        axis: Vec3::Y,
                        radius: ScalarRange::Random(0.02, 0.12),
                    }),
                    InitModule::SetVelocity(VelocityMode::Cone {
                        direction: Vec3::Y,
                        angle: 0.25,
                        speed: ScalarRange::Random(1.5, 3.0),
                    }),
                ],
                update: vec![
                    UpdateModule::Gravity(Vec3::new(0.0, 1.0, 0.0)),
                    UpdateModule::Drag(1.8),
                    UpdateModule::Noise {
                        strength: 0.7,
                        frequency: 2.5,
                        scroll: Vec3::new(0.2, 3.0, 0.2),
                    },
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(1.2, 0.7, 0.1, 0.8) },
                            GradientKey { time: 0.4, color: LinearRgba::new(0.9, 0.3, 0.02, 0.5) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.3, 0.06, 0.0, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.08, interp: Interp::Linear },
                            CurveKey { time: 0.3, value: 0.14, interp: Interp::EaseOut },
                            CurveKey { time: 1.0, value: 0.03, interp: Interp::EaseIn },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/flame_06.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
            // Embers — small glowing particles drifting upward
            EmitterDef {
                name: "Embers".to_string(),
                enabled: true,
                capacity: 128,
                spawn: SpawnModule::Rate(8.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(2.0, 4.0)),
                    InitModule::SetPosition(ShapeEmitter::Sphere {
                        center: Vec3::new(0.0, 0.3, 0.0),
                        radius: ScalarRange::Random(0.1, 0.3),
                    }),
                    InitModule::SetVelocity(VelocityMode::Cone {
                        direction: Vec3::Y,
                        angle: 0.5,
                        speed: ScalarRange::Random(0.5, 1.5),
                    }),
                ],
                update: vec![
                    UpdateModule::Gravity(Vec3::new(0.0, 0.3, 0.0)),
                    UpdateModule::Noise {
                        strength: 0.6,
                        frequency: 0.5,
                        scroll: Vec3::new(0.1, 0.3, 0.1),
                    },
                    UpdateModule::Drag(0.2),
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(1.0, 0.5, 0.0, 1.0) },
                            GradientKey { time: 0.5, color: LinearRgba::new(0.8, 0.2, 0.0, 0.6) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.3, 0.05, 0.0, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.03, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 0.01, interp: Interp::Linear },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/spark_04.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Additive,
            },
            // Smoke — soft billowing smoke above the flames
            EmitterDef {
                name: "Smoke".to_string(),
                enabled: true,
                capacity: 64,
                spawn: SpawnModule::Rate(6.0),
                init: vec![
                    InitModule::SetLifetime(ScalarRange::Random(3.0, 5.0)),
                    InitModule::SetPosition(ShapeEmitter::Sphere {
                        center: Vec3::new(0.0, 0.5, 0.0),
                        radius: ScalarRange::Random(0.1, 0.3),
                    }),
                    InitModule::SetVelocity(VelocityMode::Directional {
                        direction: Vec3::Y,
                        speed: ScalarRange::Random(0.3, 0.6),
                    }),
                    InitModule::SetSize(ScalarRange::Random(0.2, 0.4)),
                ],
                update: vec![
                    UpdateModule::Gravity(Vec3::new(0.0, 0.5, 0.0)),
                    UpdateModule::Drag(0.6),
                    UpdateModule::Noise {
                        strength: 0.3,
                        frequency: 0.4,
                        scroll: Vec3::new(0.1, 0.2, 0.1),
                    },
                    UpdateModule::ColorByLife(Gradient {
                        keys: vec![
                            GradientKey { time: 0.0, color: LinearRgba::new(0.3, 0.28, 0.25, 0.0) },
                            GradientKey { time: 0.1, color: LinearRgba::new(0.3, 0.28, 0.25, 0.25) },
                            GradientKey { time: 0.5, color: LinearRgba::new(0.25, 0.23, 0.2, 0.15) },
                            GradientKey { time: 1.0, color: LinearRgba::new(0.2, 0.18, 0.15, 0.0) },
                        ],
                    }),
                    UpdateModule::SizeByLife(Curve {
                        keys: vec![
                            CurveKey { time: 0.0, value: 0.2, interp: Interp::Linear },
                            CurveKey { time: 1.0, value: 0.8, interp: Interp::EaseOut },
                        ],
                    }),
                ],
                render: RenderModule::Billboard(BillboardConfig {
                    texture: Some("textures/particles/smoke_07.png".into()),
                    ..default()
                }),
                sim_space: SimSpace::Local,
                alpha_mode: VfxAlphaMode::Blend,
            },
        ],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

/// Rock debris burst: small cubes flung outward with gravity.
fn rock_debris() -> VfxSystem {
    VfxSystem {
        emitters: vec![EmitterDef {
            name: "Debris".to_string(),
            enabled: true,
            capacity: 64,
            spawn: SpawnModule::Burst {
                count: 15,
                interval: 1.5,
                max_cycles: None,
                offset: 0.0,
            },
            init: vec![
                InitModule::SetLifetime(ScalarRange::Random(1.0, 2.5)),
                InitModule::SetPosition(ShapeEmitter::Sphere {
                    center: Vec3::ZERO,
                    radius: ScalarRange::Constant(0.2),
                }),
                InitModule::SetVelocity(VelocityMode::Radial {
                    center: Vec3::ZERO,
                    speed: ScalarRange::Random(3.0, 8.0),
                }),
                InitModule::SetSize(ScalarRange::Random(0.08, 0.2)),
                InitModule::SetOrientation(OrientMode::RandomFull),
            ],
            update: vec![
                UpdateModule::Gravity(Vec3::new(0.0, -9.8, 0.0)),
                UpdateModule::Drag(0.3),
                UpdateModule::SizeByLife(Curve {
                    keys: vec![
                        CurveKey { time: 0.0, value: 1.0, interp: Interp::Linear },
                        CurveKey { time: 1.0, value: 0.6, interp: Interp::EaseIn },
                    ],
                }),
                UpdateModule::Spin { axis: Vec3::new(1.0, 0.5, 0.2), speed: 3.0 },
            ],
            render: RenderModule::Mesh(MeshParticleConfig {
                shape: MeshShape::Cube,
                material_path: None,
                base_color: LinearRgba::new(0.35, 0.3, 0.25, 1.0),
                collide: true,
                restitution: 0.3,
                cast_shadows: false,
            }),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Opaque,
        }],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}

fn snow() -> VfxSystem {
    VfxSystem {
        emitters: vec![EmitterDef {
            name: "Snowflakes".to_string(),
            enabled: true,
            capacity: 1024,
            spawn: SpawnModule::Rate(40.0),
            init: vec![
                InitModule::SetLifetime(ScalarRange::Random(4.0, 8.0)),
                InitModule::SetPosition(ShapeEmitter::Sphere {
                    center: Vec3::new(0.0, 5.0, 0.0),
                    radius: ScalarRange::Random(2.0, 5.0),
                }),
                InitModule::SetVelocity(VelocityMode::Radial {
                    center: Vec3::ZERO,
                    speed: ScalarRange::Random(0.1, 0.3),
                }),
            ],
            update: vec![
                UpdateModule::Gravity(Vec3::new(0.0, -0.8, 0.0)),
                UpdateModule::Drag(2.0),
                UpdateModule::ColorByLife(Gradient {
                    keys: vec![
                        GradientKey {
                            time: 0.0,
                            color: LinearRgba::new(1.0, 1.0, 1.0, 0.0),
                        },
                        GradientKey {
                            time: 0.1,
                            color: LinearRgba::new(1.0, 1.0, 1.0, 0.8),
                        },
                        GradientKey {
                            time: 0.8,
                            color: LinearRgba::new(0.9, 0.95, 1.0, 0.6),
                        },
                        GradientKey {
                            time: 1.0,
                            color: LinearRgba::new(0.8, 0.9, 1.0, 0.0),
                        },
                    ],
                }),
                UpdateModule::SizeByLife(Curve {
                    keys: vec![
                        CurveKey { time: 0.0, value: 0.04, interp: Interp::Linear },
                        CurveKey { time: 1.0, value: 0.03, interp: Interp::Linear },
                    ],
                }),
            ],
            render: RenderModule::Billboard(BillboardConfig {
                texture: Some("textures/particles/star_01.png".into()),
                ..default()
            }),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Blend,
        }],
        params: Vec::new(),
        duration: 0.0,
        looping: true,
    }
}
