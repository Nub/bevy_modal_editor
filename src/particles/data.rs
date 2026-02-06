//! Serializable data model for particle effects.
//!
//! These types mirror bevy_hanabi's API but use concrete values instead of
//! `ExprHandle`, enabling RON serialization and editor UI editing. The `build`
//! module converts them into actual `EffectAsset` instances at runtime.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Marker component
// ---------------------------------------------------------------------------

/// Serializable marker component storing a complete particle effect definition.
///
/// Runtime components (`ParticleEffect`, compiled shader data) are regenerated
/// from this marker after scene restore, following the same pattern as
/// `SceneLightMarker` → `PointLight`.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct ParticleEffectMarker {
    /// Maximum number of concurrent particles.
    pub capacity: u32,
    /// Spawner / emission configuration.
    pub spawner: SpawnerConfig,
    /// Simulation space (Global or Local).
    pub simulation_space: ParticleSimSpace,
    /// Simulation condition (WhenVisible or Always).
    pub simulation_condition: ParticleSimCondition,
    /// Motion integration mode.
    pub motion_integration: ParticleMotionIntegration,
    /// Alpha / blending mode.
    pub alpha_mode: ParticleAlphaMode,
    /// Modifiers applied once when a particle spawns.
    pub init_modifiers: Vec<InitModifierData>,
    /// Modifiers applied every frame to each living particle.
    pub update_modifiers: Vec<UpdateModifierData>,
    /// Modifiers controlling visual output (color, size, orientation).
    pub render_modifiers: Vec<RenderModifierData>,
}

impl Default for ParticleEffectMarker {
    fn default() -> Self {
        Self {
            capacity: 1024,
            spawner: SpawnerConfig::Rate { rate: 50.0 },
            simulation_space: ParticleSimSpace::Global,
            simulation_condition: ParticleSimCondition::WhenVisible,
            motion_integration: ParticleMotionIntegration::PostUpdate,
            alpha_mode: ParticleAlphaMode::Blend,
            init_modifiers: vec![
                InitModifierData::SetLifetime(ScalarRange::Constant(5.0)),
                InitModifierData::SetPositionSphere {
                    center: Vec3::ZERO,
                    radius: ScalarRange::Constant(0.1),
                    volume: false,
                },
                InitModifierData::SetVelocitySphere {
                    center: Vec3::ZERO,
                    speed: ScalarRange::Constant(2.0),
                },
            ],
            update_modifiers: vec![AccelModifierData {
                accel: Vec3::new(0.0, -9.8, 0.0),
            }
            .into()],
            render_modifiers: vec![
                RenderModifierData::ColorOverLifetime {
                    keys: vec![
                        GradientKeyData {
                            ratio: 0.0,
                            value: Vec4::new(1.0, 0.8, 0.2, 1.0),
                        },
                        GradientKeyData {
                            ratio: 1.0,
                            value: Vec4::new(1.0, 0.0, 0.0, 0.0),
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
                            value: Vec4::new(0.0, 0.0, 0.0, 0.0),
                        },
                    ],
                },
            ],
        }
    }
}

// ---------------------------------------------------------------------------
// Scalar / vector value types
// ---------------------------------------------------------------------------

/// A scalar value that can be constant or a random range.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Reflect)]
pub enum ScalarRange {
    Constant(f32),
    Random(f32, f32),
}

impl Default for ScalarRange {
    fn default() -> Self {
        Self::Constant(1.0)
    }
}

// ---------------------------------------------------------------------------
// Spawner configuration
// ---------------------------------------------------------------------------

/// Serializable spawner configuration.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub enum SpawnerConfig {
    /// Continuous stream at `rate` particles per second.
    Rate { rate: f32 },
    /// Single burst of `count` particles.
    Once { count: f32 },
    /// Repeated bursts of `count` particles every `period` seconds.
    Burst { count: f32, period: f32 },
}

impl Default for SpawnerConfig {
    fn default() -> Self {
        Self::Rate { rate: 50.0 }
    }
}

// ---------------------------------------------------------------------------
// Enums mirroring bevy_hanabi types
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum ParticleSimSpace {
    #[default]
    Global,
    Local,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum ParticleSimCondition {
    #[default]
    WhenVisible,
    Always,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum ParticleMotionIntegration {
    None,
    PreUpdate,
    #[default]
    PostUpdate,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum ParticleAlphaMode {
    #[default]
    Blend,
    Premultiply,
    Add,
    Multiply,
    Opaque,
}

impl ParticleAlphaMode {
    pub const ALL: [Self; 5] = [
        Self::Blend,
        Self::Premultiply,
        Self::Add,
        Self::Multiply,
        Self::Opaque,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Blend => "Blend",
            Self::Premultiply => "Premultiply",
            Self::Add => "Additive",
            Self::Multiply => "Multiply",
            Self::Opaque => "Opaque",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum ParticleOrientMode {
    #[default]
    ParallelCameraDepthPlane,
    FaceCameraPosition,
    AlongVelocity,
}

impl ParticleOrientMode {
    pub const ALL: [Self; 3] = [
        Self::ParallelCameraDepthPlane,
        Self::FaceCameraPosition,
        Self::AlongVelocity,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::ParallelCameraDepthPlane => "Billboard",
            Self::FaceCameraPosition => "Face Camera",
            Self::AlongVelocity => "Along Velocity",
        }
    }
}

// ---------------------------------------------------------------------------
// Gradient key
// ---------------------------------------------------------------------------

/// A single gradient keypoint. `value` holds RGBA for color gradients or
/// XYZ (with W unused) for size gradients.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct GradientKeyData {
    pub ratio: f32,
    pub value: Vec4,
}

// ---------------------------------------------------------------------------
// Init modifiers
// ---------------------------------------------------------------------------

/// Serializable init modifier (applied once at particle spawn).
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub enum InitModifierData {
    /// Set particle lifetime.
    SetLifetime(ScalarRange),
    /// Set initial color (RGBA as Vec4).
    SetColor(Vec4),
    /// Set initial size (uniform).
    SetSize(ScalarRange),
    /// Spawn on/in a sphere.
    SetPositionSphere {
        center: Vec3,
        radius: ScalarRange,
        /// true = Volume, false = Surface
        volume: bool,
    },
    /// Spawn on/in a circle.
    SetPositionCircle {
        center: Vec3,
        axis: Vec3,
        radius: ScalarRange,
        volume: bool,
    },
    /// Radial velocity away from center.
    SetVelocitySphere {
        center: Vec3,
        speed: ScalarRange,
    },
    /// Tangent velocity around axis.
    SetVelocityTangent {
        origin: Vec3,
        axis: Vec3,
        speed: ScalarRange,
    },
}

impl InitModifierData {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SetLifetime(_) => "Lifetime",
            Self::SetColor(_) => "Color",
            Self::SetSize(_) => "Size",
            Self::SetPositionSphere { .. } => "Position Sphere",
            Self::SetPositionCircle { .. } => "Position Circle",
            Self::SetVelocitySphere { .. } => "Velocity Sphere",
            Self::SetVelocityTangent { .. } => "Velocity Tangent",
        }
    }

    pub const ADD_OPTIONS: &[(&str, fn() -> Self)] = &[
        ("Lifetime", || Self::SetLifetime(ScalarRange::Constant(5.0))),
        ("Color", || Self::SetColor(Vec4::ONE)),
        ("Size", || Self::SetSize(ScalarRange::Constant(0.1))),
        ("Position Sphere", || Self::SetPositionSphere {
            center: Vec3::ZERO,
            radius: ScalarRange::Constant(0.5),
            volume: false,
        }),
        ("Position Circle", || Self::SetPositionCircle {
            center: Vec3::ZERO,
            axis: Vec3::Y,
            radius: ScalarRange::Constant(0.5),
            volume: false,
        }),
        ("Velocity Sphere", || Self::SetVelocitySphere {
            center: Vec3::ZERO,
            speed: ScalarRange::Constant(2.0),
        }),
        ("Velocity Tangent", || Self::SetVelocityTangent {
            origin: Vec3::ZERO,
            axis: Vec3::Y,
            speed: ScalarRange::Constant(2.0),
        }),
    ];
}

// ---------------------------------------------------------------------------
// Update modifiers
// ---------------------------------------------------------------------------

/// Constant acceleration (e.g. gravity).
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct AccelModifierData {
    pub accel: Vec3,
}

/// Radial acceleration toward/away from origin.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct RadialAccelData {
    pub origin: Vec3,
    pub accel: f32,
}

/// Linear drag (deceleration proportional to speed).
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct LinearDragData {
    pub drag: f32,
}

/// Kill particles outside (or inside) an AABB.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct KillAabbData {
    pub center: Vec3,
    pub half_size: Vec3,
    pub kill_inside: bool,
}

/// Kill particles outside (or inside) a sphere.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct KillSphereData {
    pub center: Vec3,
    pub radius: f32,
    pub kill_inside: bool,
}

/// Tangential acceleration (orbital/swirl motion around an axis).
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct TangentAccelData {
    pub origin: Vec3,
    pub axis: Vec3,
    pub accel: f32,
}

/// Attract particles toward a sphere surface.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct ConformToSphereData {
    pub origin: Vec3,
    pub radius: f32,
    pub influence_dist: f32,
    pub attraction_accel: f32,
    pub max_speed: f32,
}

/// Constrain particle position to a cone volume or surface.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct SetPositionCone3dData {
    pub height: f32,
    pub base_radius: f32,
    pub top_radius: f32,
    pub volume: bool,
}

/// Serializable update modifier (applied every frame).
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub enum UpdateModifierData {
    Accel(AccelModifierData),
    RadialAccel(RadialAccelData),
    LinearDrag(LinearDragData),
    KillAabb(KillAabbData),
    KillSphere(KillSphereData),
    TangentAccel(TangentAccelData),
    ConformToSphere(ConformToSphereData),
    SetPositionCone3d(SetPositionCone3dData),
}

impl From<AccelModifierData> for UpdateModifierData {
    fn from(v: AccelModifierData) -> Self {
        Self::Accel(v)
    }
}

impl UpdateModifierData {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Accel(_) => "Acceleration",
            Self::RadialAccel(_) => "Radial Accel",
            Self::LinearDrag(_) => "Linear Drag",
            Self::KillAabb(_) => "Kill AABB",
            Self::KillSphere(_) => "Kill Sphere",
            Self::TangentAccel(_) => "Tangent Accel",
            Self::ConformToSphere(_) => "Conform to Sphere",
            Self::SetPositionCone3d(_) => "Position Cone 3D",
        }
    }

    pub const ADD_OPTIONS: &[(&str, fn() -> Self)] = &[
        ("Acceleration", || Self::Accel(AccelModifierData { accel: Vec3::new(0.0, -9.8, 0.0) })),
        ("Radial Accel", || Self::RadialAccel(RadialAccelData { origin: Vec3::ZERO, accel: 1.0 })),
        ("Linear Drag", || Self::LinearDrag(LinearDragData { drag: 1.0 })),
        ("Kill AABB", || Self::KillAabb(KillAabbData {
            center: Vec3::ZERO,
            half_size: Vec3::splat(10.0),
            kill_inside: false,
        })),
        ("Kill Sphere", || Self::KillSphere(KillSphereData {
            center: Vec3::ZERO,
            radius: 10.0,
            kill_inside: false,
        })),
        ("Tangent Accel", || Self::TangentAccel(TangentAccelData {
            origin: Vec3::ZERO,
            axis: Vec3::Y,
            accel: 5.0,
        })),
        ("Conform to Sphere", || Self::ConformToSphere(ConformToSphereData {
            origin: Vec3::ZERO,
            radius: 2.0,
            influence_dist: 5.0,
            attraction_accel: 10.0,
            max_speed: 5.0,
        })),
        ("Position Cone 3D", || Self::SetPositionCone3d(SetPositionCone3dData {
            height: 2.0,
            base_radius: 1.0,
            top_radius: 0.0,
            volume: true,
        })),
    ];
}

// ---------------------------------------------------------------------------
// Render modifiers
// ---------------------------------------------------------------------------

/// Serializable render modifier (controls visual output).
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub enum RenderModifierData {
    /// Color gradient over particle lifetime.
    ColorOverLifetime { keys: Vec<GradientKeyData> },
    /// Size gradient over particle lifetime.
    SizeOverLifetime { keys: Vec<GradientKeyData> },
    /// Constant color (RGBA).
    SetColor { color: Vec4 },
    /// Constant size (XYZ).
    SetSize { size: Vec3 },
    /// Particle orientation / billboard mode.
    Orient { mode: ParticleOrientMode },
    /// Render particles as screen-space sized.
    ScreenSpaceSize,
}

impl RenderModifierData {
    pub fn label(&self) -> &'static str {
        match self {
            Self::ColorOverLifetime { .. } => "Color Over Lifetime",
            Self::SizeOverLifetime { .. } => "Size Over Lifetime",
            Self::SetColor { .. } => "Set Color",
            Self::SetSize { .. } => "Set Size",
            Self::Orient { .. } => "Orient",
            Self::ScreenSpaceSize => "Screen Space Size",
        }
    }

    pub const ADD_OPTIONS: &[(&str, fn() -> Self)] = &[
        ("Color Over Lifetime", || Self::ColorOverLifetime {
            keys: vec![
                GradientKeyData { ratio: 0.0, value: Vec4::ONE },
                GradientKeyData { ratio: 1.0, value: Vec4::new(1.0, 1.0, 1.0, 0.0) },
            ],
        }),
        ("Size Over Lifetime", || Self::SizeOverLifetime {
            keys: vec![
                GradientKeyData { ratio: 0.0, value: Vec4::new(0.1, 0.1, 0.1, 0.0) },
                GradientKeyData { ratio: 1.0, value: Vec4::ZERO },
            ],
        }),
        ("Set Color", || Self::SetColor { color: Vec4::ONE }),
        ("Set Size", || Self::SetSize { size: Vec3::splat(0.1) }),
        ("Orient (Billboard)", || Self::Orient { mode: ParticleOrientMode::ParallelCameraDepthPlane }),
        ("Screen Space Size", || Self::ScreenSpaceSize),
    ];
}
