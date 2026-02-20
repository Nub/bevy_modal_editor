//! Core data model for the VFX system.
//!
//! All types are serializable (serde + RON) and reflectable (Bevy Reflect).
//! The GPU compute pipeline reads these definitions and uploads packed parameter
//! buffers — the CPU never touches individual particle data.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::curve::{Curve, Gradient};

// ---------------------------------------------------------------------------
// Top-level VFX system
// ---------------------------------------------------------------------------

/// Top-level VFX system component. Groups one or more emitters with shared
/// parameters and playback settings.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
#[reflect(Component, Default)]
pub struct VfxSystem {
    /// Ordered list of emitter definitions.
    pub emitters: Vec<EmitterDef>,
    /// Exposed parameters (future: bindable from game code).
    pub params: Vec<VfxParam>,
    /// Total duration in seconds. 0.0 = infinite.
    pub duration: f32,
    /// Whether the system loops after `duration` elapses.
    pub looping: bool,
}

impl Default for VfxSystem {
    fn default() -> Self {
        Self {
            emitters: vec![EmitterDef::default()],
            params: Vec::new(),
            duration: 0.0,
            looping: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Emitter definition
// ---------------------------------------------------------------------------

/// A single emitter within a VFX system.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub struct EmitterDef {
    /// Display name (for editor UI).
    pub name: String,
    /// Whether this emitter is active.
    pub enabled: bool,
    /// Maximum number of concurrent particles.
    pub capacity: u32,
    /// Spawn / emission configuration.
    pub spawn: SpawnModule,
    /// Modifiers applied once at particle birth (ordered stack).
    pub init: Vec<InitModule>,
    /// Modifiers applied every frame (ordered stack).
    pub update: Vec<UpdateModule>,
    /// How particles are rendered (one mode per emitter).
    pub render: RenderModule,
    /// Simulation space.
    pub sim_space: SimSpace,
    /// Alpha / blending mode.
    pub alpha_mode: VfxAlphaMode,
}

impl Default for EmitterDef {
    fn default() -> Self {
        Self {
            name: "Emitter".to_string(),
            enabled: true,
            capacity: 1024,
            spawn: SpawnModule::Rate(50.0),
            init: vec![
                InitModule::SetLifetime(ScalarRange::Constant(5.0)),
                InitModule::SetPosition(ShapeEmitter::Sphere {
                    center: Vec3::ZERO,
                    radius: ScalarRange::Constant(0.1),
                }),
                InitModule::SetVelocity(VelocityMode::Radial {
                    center: Vec3::ZERO,
                    speed: ScalarRange::Constant(2.0),
                }),
            ],
            update: vec![UpdateModule::Gravity(Vec3::new(0.0, -9.8, 0.0))],
            render: RenderModule::Billboard(BillboardConfig::default()),
            sim_space: SimSpace::Local,
            alpha_mode: VfxAlphaMode::Blend,
        }
    }
}

// ---------------------------------------------------------------------------
// Simulation space
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum SimSpace {
    /// Particles simulate in world space.
    World,
    /// Particles simulate in the emitter's local space.
    #[default]
    Local,
}

impl SimSpace {
    pub const ALL: [Self; 2] = [Self::World, Self::Local];

    pub fn label(&self) -> &'static str {
        match self {
            Self::World => "World",
            Self::Local => "Local",
        }
    }
}

// ---------------------------------------------------------------------------
// Alpha / blending mode
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum VfxAlphaMode {
    #[default]
    Blend,
    Additive,
    Premultiply,
    Multiply,
    Opaque,
}

impl VfxAlphaMode {
    pub const ALL: [Self; 5] = [
        Self::Blend,
        Self::Additive,
        Self::Premultiply,
        Self::Multiply,
        Self::Opaque,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Blend => "Blend",
            Self::Additive => "Additive",
            Self::Premultiply => "Premultiply",
            Self::Multiply => "Multiply",
            Self::Opaque => "Opaque",
        }
    }

    /// Convert to Bevy's `AlphaMode` for use with `StandardMaterial`.
    pub fn to_bevy(self) -> AlphaMode {
        match self {
            Self::Blend => AlphaMode::Blend,
            Self::Additive => AlphaMode::Add,
            Self::Premultiply => AlphaMode::Premultiplied,
            Self::Multiply => AlphaMode::Multiply,
            Self::Opaque => AlphaMode::Opaque,
        }
    }
}

// ---------------------------------------------------------------------------
// Scalar / vector value types
// ---------------------------------------------------------------------------

/// A scalar value that can be constant or a random range.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Reflect)]
pub enum ScalarRange {
    Constant(f32),
    Random(f32, f32),
}

impl Default for ScalarRange {
    fn default() -> Self {
        Self::Constant(1.0)
    }
}

impl ScalarRange {
    /// Get the minimum value.
    pub fn min_val(&self) -> f32 {
        match self {
            Self::Constant(v) => *v,
            Self::Random(a, _) => *a,
        }
    }

    /// Get the maximum value.
    pub fn max_val(&self) -> f32 {
        match self {
            Self::Constant(v) => *v,
            Self::Random(_, b) => *b,
        }
    }

    /// Sample a value from this range using CPU-side randomness.
    pub fn sample(&self) -> f32 {
        match self {
            Self::Constant(v) => *v,
            Self::Random(a, b) => *a + (*b - *a) * fastrand::f32(),
        }
    }
}

// ---------------------------------------------------------------------------
// Spawn module
// ---------------------------------------------------------------------------

/// Controls how many particles an emitter spawns per frame.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub enum SpawnModule {
    /// Continuous stream at N particles per second.
    Rate(f32),
    /// Repeated bursts.
    Burst {
        count: u32,
        interval: f32,
        max_cycles: Option<u32>,
    },
    /// Single burst on activation.
    Once(u32),
    /// Spawn along movement path with given spacing.
    Distance { spacing: f32 },
}

impl Default for SpawnModule {
    fn default() -> Self {
        Self::Rate(50.0)
    }
}

impl SpawnModule {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Rate(_) => "Rate",
            Self::Burst { .. } => "Burst",
            Self::Once(_) => "Once",
            Self::Distance { .. } => "Distance",
        }
    }
}

// ---------------------------------------------------------------------------
// Shape emitters (used by InitModule::SetPosition)
// ---------------------------------------------------------------------------

/// Shape from which particles are emitted.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub enum ShapeEmitter {
    /// Emit from a sphere (surface or volume).
    Sphere {
        center: Vec3,
        radius: ScalarRange,
    },
    /// Emit from a box (surface or volume).
    Box {
        center: Vec3,
        half_extents: Vec3,
    },
    /// Emit from a cone.
    Cone {
        angle: f32,
        radius: f32,
        height: f32,
    },
    /// Emit from a circle (surface or filled).
    Circle {
        center: Vec3,
        axis: Vec3,
        radius: ScalarRange,
    },
    /// Emit from a line segment.
    Edge {
        start: Vec3,
        end: Vec3,
    },
    /// Emit from a single point.
    Point(Vec3),
}

impl Default for ShapeEmitter {
    fn default() -> Self {
        Self::Sphere {
            center: Vec3::ZERO,
            radius: ScalarRange::Constant(0.1),
        }
    }
}

impl ShapeEmitter {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Sphere { .. } => "Sphere",
            Self::Box { .. } => "Box",
            Self::Cone { .. } => "Cone",
            Self::Circle { .. } => "Circle",
            Self::Edge { .. } => "Edge",
            Self::Point(_) => "Point",
        }
    }
}

// ---------------------------------------------------------------------------
// Velocity modes (used by InitModule::SetVelocity)
// ---------------------------------------------------------------------------

/// How initial velocity is determined.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub enum VelocityMode {
    /// Radial outward from center.
    Radial { center: Vec3, speed: ScalarRange },
    /// Fixed direction.
    Directional { direction: Vec3, speed: ScalarRange },
    /// Tangent around an axis (orbital).
    Tangent { axis: Vec3, speed: ScalarRange },
    /// Random direction within a cone.
    Cone { direction: Vec3, angle: f32, speed: ScalarRange },
    /// Fully random direction and speed.
    Random { speed: ScalarRange },
}

impl Default for VelocityMode {
    fn default() -> Self {
        Self::Radial {
            center: Vec3::ZERO,
            speed: ScalarRange::Constant(2.0),
        }
    }
}

impl VelocityMode {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Radial { .. } => "Radial",
            Self::Directional { .. } => "Directional",
            Self::Tangent { .. } => "Tangent",
            Self::Cone { .. } => "Cone",
            Self::Random { .. } => "Random",
        }
    }
}

// ---------------------------------------------------------------------------
// Color source (used by InitModule::SetColor)
// ---------------------------------------------------------------------------

/// How initial color is determined.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub enum ColorSource {
    /// Fixed color.
    Constant(LinearRgba),
    /// Random from gradient.
    RandomFromGradient(Gradient),
}

impl Default for ColorSource {
    fn default() -> Self {
        Self::Constant(LinearRgba::WHITE)
    }
}

// ---------------------------------------------------------------------------
// Orientation mode (used by InitModule::SetOrientation)
// ---------------------------------------------------------------------------

/// How mesh particle orientation is determined at spawn.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum OrientMode {
    /// No rotation.
    #[default]
    Identity,
    /// Random rotation around Y axis.
    RandomY,
    /// Fully random rotation (all axes).
    RandomFull,
    /// Align mesh Z-axis to velocity direction.
    AlignVelocity,
    /// Billboard: always face the camera.
    FaceCamera,
}

impl OrientMode {
    pub const ALL: [Self; 5] = [Self::Identity, Self::RandomY, Self::RandomFull, Self::AlignVelocity, Self::FaceCamera];

    pub fn label(&self) -> &'static str {
        match self {
            Self::Identity => "Identity",
            Self::RandomY => "Random Y",
            Self::RandomFull => "Random Full",
            Self::AlignVelocity => "Align Velocity",
            Self::FaceCamera => "Face Camera",
        }
    }
}

// ---------------------------------------------------------------------------
// Init modules (applied once at particle birth)
// ---------------------------------------------------------------------------

/// Modifier applied once when a particle spawns.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub enum InitModule {
    /// Set particle lifetime.
    SetLifetime(ScalarRange),
    /// Set initial position from shape emitter.
    SetPosition(ShapeEmitter),
    /// Set initial velocity.
    SetVelocity(VelocityMode),
    /// Set initial color.
    SetColor(ColorSource),
    /// Set initial size (uniform).
    SetSize(ScalarRange),
    /// Set initial rotation (radians, billboard/2D).
    SetRotation(ScalarRange),
    /// Set initial 3D orientation (mesh particles).
    SetOrientation(OrientMode),
    /// Set non-uniform initial scale (per-axis).
    SetScale3d {
        x: ScalarRange,
        y: ScalarRange,
        z: ScalarRange,
    },
    /// Set UV tiling scale (emitter-level, affects shared material).
    SetUvScale([f32; 2]),
    /// Inherit velocity from emitter movement.
    InheritVelocity { ratio: f32 },
}

impl InitModule {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SetLifetime(_) => "Lifetime",
            Self::SetPosition(_) => "Position",
            Self::SetVelocity(_) => "Velocity",
            Self::SetColor(_) => "Color",
            Self::SetSize(_) => "Size",
            Self::SetRotation(_) => "Rotation",
            Self::SetOrientation(_) => "Orientation",
            Self::SetScale3d { .. } => "Scale 3D",
            Self::SetUvScale(_) => "UV Scale",
            Self::InheritVelocity { .. } => "Inherit Velocity",
        }
    }

    pub const ADD_OPTIONS: &[(&str, fn() -> Self)] = &[
        ("Lifetime", || Self::SetLifetime(ScalarRange::Constant(5.0))),
        ("Position", || Self::SetPosition(ShapeEmitter::default())),
        ("Velocity", || Self::SetVelocity(VelocityMode::default())),
        ("Color", || Self::SetColor(ColorSource::default())),
        ("Size", || Self::SetSize(ScalarRange::Constant(0.1))),
        ("Rotation", || Self::SetRotation(ScalarRange::Constant(0.0))),
        ("Orientation", || Self::SetOrientation(OrientMode::default())),
        ("Scale 3D", || Self::SetScale3d {
            x: ScalarRange::Constant(1.0),
            y: ScalarRange::Constant(1.0),
            z: ScalarRange::Constant(1.0),
        }),
        ("UV Scale", || Self::SetUvScale([1.0, 1.0])),
        ("Inherit Velocity", || Self::InheritVelocity { ratio: 1.0 }),
    ];
}

// ---------------------------------------------------------------------------
// Update modules (applied every frame)
// ---------------------------------------------------------------------------

/// Modifier applied every frame to each living particle.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub enum UpdateModule {
    /// Constant acceleration (e.g. gravity).
    Gravity(Vec3),
    /// Constant force.
    ConstantForce(Vec3),
    /// Linear drag (deceleration proportional to speed).
    Drag(f32),
    /// Curl noise displacement.
    Noise {
        strength: f32,
        frequency: f32,
        scroll: Vec3,
    },
    /// Orbit around an axis.
    OrbitAround {
        axis: Vec3,
        speed: f32,
        radius_decay: f32,
    },
    /// Attract toward a point.
    Attract {
        target: Vec3,
        strength: f32,
        falloff: f32,
    },
    /// Kill particles inside/outside a shape.
    KillZone {
        shape: KillShape,
        invert: bool,
    },
    /// Size over normalized lifetime.
    SizeByLife(Curve<f32>),
    /// Color over normalized lifetime.
    ColorByLife(Gradient),
    /// Size scaled by speed.
    SizeBySpeed {
        min_speed: f32,
        max_speed: f32,
        min_size: f32,
        max_size: f32,
    },
    /// Rotate particles to align with velocity direction.
    RotateByVelocity,
    /// Tangent acceleration (orbital/swirl).
    TangentAccel {
        origin: Vec3,
        axis: Vec3,
        accel: f32,
    },
    /// Radial acceleration toward/away from origin.
    RadialAccel {
        origin: Vec3,
        accel: f32,
    },
    /// Continuous rotation around an axis.
    Spin {
        axis: Vec3,
        speed: f32,
    },
    /// UV scrolling animation (emitter-level, affects shared material).
    UvScroll {
        speed: [f32; 2],
    },
    /// Per-axis scale over normalized lifetime.
    Scale3dByLife {
        x: Curve<f32>,
        y: Curve<f32>,
        z: Curve<f32>,
    },
    /// Per-axis position offset over normalized lifetime.
    OffsetByLife {
        x: Curve<f32>,
        y: Curve<f32>,
        z: Curve<f32>,
    },
    /// Emissive color over normalized lifetime (mesh particles only).
    EmissiveOverLife(Gradient),
}

impl UpdateModule {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Gravity(_) => "Gravity",
            Self::ConstantForce(_) => "Constant Force",
            Self::Drag(_) => "Drag",
            Self::Noise { .. } => "Noise",
            Self::OrbitAround { .. } => "Orbit",
            Self::Attract { .. } => "Attract",
            Self::KillZone { .. } => "Kill Zone",
            Self::SizeByLife(_) => "Size Over Life",
            Self::ColorByLife(_) => "Color Over Life",
            Self::SizeBySpeed { .. } => "Size By Speed",
            Self::RotateByVelocity => "Rotate By Velocity",
            Self::TangentAccel { .. } => "Tangent Accel",
            Self::RadialAccel { .. } => "Radial Accel",
            Self::Spin { .. } => "Spin",
            Self::UvScroll { .. } => "UV Scroll",
            Self::Scale3dByLife { .. } => "Scale 3D Over Life",
            Self::OffsetByLife { .. } => "Offset Over Life",
            Self::EmissiveOverLife(_) => "Emissive Over Life",
        }
    }

    pub const ADD_OPTIONS: &[(&str, fn() -> Self)] = &[
        ("Gravity", || Self::Gravity(Vec3::new(0.0, -9.8, 0.0))),
        ("Constant Force", || Self::ConstantForce(Vec3::ZERO)),
        ("Drag", || Self::Drag(1.0)),
        ("Noise", || Self::Noise {
            strength: 1.0,
            frequency: 1.0,
            scroll: Vec3::new(0.0, 1.0, 0.0),
        }),
        ("Orbit", || Self::OrbitAround {
            axis: Vec3::Y,
            speed: 2.0,
            radius_decay: 0.0,
        }),
        ("Attract", || Self::Attract {
            target: Vec3::ZERO,
            strength: 5.0,
            falloff: 1.0,
        }),
        ("Kill Zone (Sphere)", || Self::KillZone {
            shape: KillShape::Sphere {
                center: Vec3::ZERO,
                radius: 10.0,
            },
            invert: false,
        }),
        ("Size Over Life", || Self::SizeByLife(Curve::linear(1.0, 0.0))),
        ("Color Over Life", || Self::ColorByLife(Gradient::white_to_transparent())),
        ("Size By Speed", || Self::SizeBySpeed {
            min_speed: 0.0,
            max_speed: 10.0,
            min_size: 0.01,
            max_size: 1.0,
        }),
        ("Rotate By Velocity", || Self::RotateByVelocity),
        ("Spin", || Self::Spin {
            axis: Vec3::Y,
            speed: 2.0,
        }),
        ("Tangent Accel", || Self::TangentAccel {
            origin: Vec3::ZERO,
            axis: Vec3::Y,
            accel: 5.0,
        }),
        ("Radial Accel", || Self::RadialAccel {
            origin: Vec3::ZERO,
            accel: -2.0,
        }),
        ("UV Scroll", || Self::UvScroll {
            speed: [0.0, 0.5],
        }),
        ("Scale 3D Over Life", || Self::Scale3dByLife {
            x: Curve::constant(1.0),
            y: Curve::constant(1.0),
            z: Curve::constant(1.0),
        }),
        ("Offset Over Life", || Self::OffsetByLife {
            x: Curve::constant(0.0),
            y: Curve::linear(0.0, 1.0),
            z: Curve::constant(0.0),
        }),
        ("Emissive Over Life", || Self::EmissiveOverLife(Gradient::constant(LinearRgba::BLACK))),
    ];
}

// ---------------------------------------------------------------------------
// Kill shapes
// ---------------------------------------------------------------------------

/// Shape used for kill zone testing.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub enum KillShape {
    Sphere { center: Vec3, radius: f32 },
    Box { center: Vec3, half_extents: Vec3 },
}

// ---------------------------------------------------------------------------
// Render modules
// ---------------------------------------------------------------------------

/// How particles are rendered. Each emitter has exactly one render mode.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub enum RenderModule {
    Billboard(BillboardConfig),
    Ribbon(RibbonConfig),
    Mesh(MeshParticleConfig),
}

impl RenderModule {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Billboard(_) => "Billboard",
            Self::Ribbon(_) => "Ribbon",
            Self::Mesh(_) => "Mesh",
        }
    }
}

/// Billboard / camera-facing quad rendering configuration.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub struct BillboardConfig {
    /// Billboard orientation mode.
    pub orient: BillboardOrient,
    /// Texture asset path (if any).
    pub texture: Option<String>,
    /// Flipbook animation settings (if any).
    pub flipbook: Option<FlipbookConfig>,
    /// Distance for soft particle depth fade (0 = disabled).
    pub soft_particle_distance: f32,
}

impl Default for BillboardConfig {
    fn default() -> Self {
        Self {
            orient: BillboardOrient::FaceCamera,
            texture: None,
            flipbook: None,
            soft_particle_distance: 0.0,
        }
    }
}

/// How billboards orient relative to the camera.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum BillboardOrient {
    /// Face camera position (true billboard).
    #[default]
    FaceCamera,
    /// Parallel to camera depth plane.
    ParallelCamera,
    /// Align long axis to velocity direction.
    AlongVelocity,
}

impl BillboardOrient {
    pub const ALL: [Self; 3] = [Self::FaceCamera, Self::ParallelCamera, Self::AlongVelocity];

    pub fn label(&self) -> &'static str {
        match self {
            Self::FaceCamera => "Face Camera",
            Self::ParallelCamera => "Parallel Camera",
            Self::AlongVelocity => "Along Velocity",
        }
    }
}

/// Flipbook (spritesheet) animation configuration.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub struct FlipbookConfig {
    pub rows: u32,
    pub columns: u32,
    pub fps: f32,
}

/// Ribbon / trail rendering configuration.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub struct RibbonConfig {
    /// Width over normalized particle lifetime.
    pub width_curve: Curve<f32>,
    /// How UVs are mapped along the ribbon.
    pub texture_mode: RibbonTextureMode,
    /// Whether the ribbon faces the camera.
    pub face_camera: bool,
    /// History samples per particle.
    pub segments_per_particle: u32,
    /// Texture asset path (if any).
    pub texture: Option<String>,
}

impl Default for RibbonConfig {
    fn default() -> Self {
        Self {
            width_curve: Curve::constant(0.1),
            texture_mode: RibbonTextureMode::Stretch,
            face_camera: true,
            segments_per_particle: 16,
            texture: None,
        }
    }
}

/// How UVs are applied to ribbon geometry.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum RibbonTextureMode {
    /// Stretch texture along ribbon length.
    #[default]
    Stretch,
    /// Tile texture along ribbon length.
    Tile,
}

/// Built-in mesh shapes for mesh particles.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq, Reflect)]
pub enum MeshShape {
    #[default]
    Cube,
    Sphere,
    Capsule,
    Cylinder,
    Quad,
    /// Custom mesh asset path.
    Custom(String),
}

impl MeshShape {
    pub const BUILTIN: [Self; 5] = [Self::Cube, Self::Sphere, Self::Capsule, Self::Cylinder, Self::Quad];

    pub fn label(&self) -> &str {
        match self {
            Self::Cube => "Cube",
            Self::Sphere => "Sphere",
            Self::Capsule => "Capsule",
            Self::Cylinder => "Cylinder",
            Self::Quad => "Quad",
            Self::Custom(path) => path.as_str(),
        }
    }
}

/// Instanced mesh per particle configuration.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub struct MeshParticleConfig {
    /// Mesh shape to use for each particle.
    pub shape: MeshShape,
    /// Material library name (optional). When set, overrides base_color.
    pub material_path: Option<String>,
    /// Fallback base color when no material_path is set.
    pub base_color: LinearRgba,
    /// Enable physics colliders so particles bounce off geometry.
    #[serde(default)]
    pub collide: bool,
    /// Bounciness (coefficient of restitution) when `collide` is true.
    #[serde(default = "default_restitution")]
    pub restitution: f32,
    /// Whether mesh particles cast shadows.
    #[serde(default)]
    pub cast_shadows: bool,
}

fn default_restitution() -> f32 {
    0.3
}

impl Default for MeshParticleConfig {
    fn default() -> Self {
        Self {
            shape: MeshShape::Cube,
            material_path: None,
            base_color: LinearRgba::new(0.5, 0.5, 0.5, 1.0),
            collide: false,
            restitution: 0.3,
            cast_shadows: false,
        }
    }
}

// ---------------------------------------------------------------------------
// Exposed parameters (future use)
// ---------------------------------------------------------------------------

/// An exposed parameter that can be bound from game code or the effect sequencer.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub struct VfxParam {
    pub name: String,
    pub value: VfxParamValue,
}

/// Parameter value types.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub enum VfxParamValue {
    Float(f32),
    Vec3(Vec3),
    Color(LinearRgba),
    Curve(Curve<f32>),
}

// ---------------------------------------------------------------------------
// VFX library (preset storage)
// ---------------------------------------------------------------------------

/// Library of named VFX presets. No disk I/O — that's the editor's job.
#[derive(Resource, Default)]
pub struct VfxLibrary {
    pub effects: std::collections::HashMap<String, VfxSystem>,
}
