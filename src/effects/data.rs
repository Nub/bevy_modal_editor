//! Data model for the effect sequencer system.
//!
//! An effect is a list of **steps**, each with a **trigger** and one or more
//! **actions**. Triggers fire based on time, collision, or internal events.
//! Actions spawn entities, apply physics, emit events, etc.

use std::collections::{HashMap, HashSet};

use avian3d::prelude::RigidBody;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::scene::PrimitiveShape;

// ---------------------------------------------------------------------------
// Core marker component (serialized to scene)
// ---------------------------------------------------------------------------

/// Serializable marker component storing a complete effect definition.
///
/// Runtime playback state (`EffectPlayback`) is regenerated from this marker
/// after scene restore, following the same pattern as other marker components.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Reflect)]
#[reflect(Component)]
pub struct EffectMarker {
    pub steps: Vec<EffectStep>,
}

impl Default for EffectMarker {
    fn default() -> Self {
        Self { steps: Vec::new() }
    }
}

// ---------------------------------------------------------------------------
// Steps
// ---------------------------------------------------------------------------

/// A single step in an effect sequence.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub struct EffectStep {
    /// Human-readable label for this step.
    pub name: String,
    /// When this step fires.
    pub trigger: EffectTrigger,
    /// What happens when this step fires.
    pub actions: Vec<EffectAction>,
}

// ---------------------------------------------------------------------------
// Triggers
// ---------------------------------------------------------------------------

/// Determines when an effect step fires.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub enum EffectTrigger {
    /// Fire at a specific time (seconds from effect start).
    AtTime(f32),
    /// Fire when a tagged entity collides with anything.
    OnCollision { tag: String },
    /// Fire when another step emits a named event.
    OnEffectEvent(String),
    /// Fire after another named rule fires, with an optional delay.
    AfterRule { source_rule: String, delay: f32 },
    /// Fire repeatedly at a fixed interval.
    RepeatingInterval { interval: f32, max_count: Option<u32> },
    /// Fire once immediately when the effect starts playing.
    OnSpawn,
    /// Fire after no other rule has fired for the given duration.
    AfterIdleTimeout { timeout: f32 },
}

impl Default for EffectTrigger {
    fn default() -> Self {
        Self::AtTime(0.0)
    }
}

impl EffectTrigger {
    pub fn label(&self) -> &'static str {
        match self {
            Self::AtTime(_) => "At Time",
            Self::OnCollision { .. } => "On Collision",
            Self::OnEffectEvent(_) => "On Effect Event",
            Self::AfterRule { .. } => "After Rule",
            Self::RepeatingInterval { .. } => "Repeating Interval",
            Self::OnSpawn => "On Spawn",
            Self::AfterIdleTimeout { .. } => "After Idle Timeout",
        }
    }

    pub fn variant_index(&self) -> usize {
        match self {
            Self::AtTime(_) => 0,
            Self::OnCollision { .. } => 1,
            Self::OnEffectEvent(_) => 2,
            Self::AfterRule { .. } => 3,
            Self::RepeatingInterval { .. } => 4,
            Self::OnSpawn => 5,
            Self::AfterIdleTimeout { .. } => 6,
        }
    }

    pub fn from_variant_index(idx: usize) -> Self {
        match idx {
            0 => Self::AtTime(0.0),
            1 => Self::OnCollision {
                tag: String::new(),
            },
            2 => Self::OnEffectEvent(String::new()),
            3 => Self::AfterRule {
                source_rule: String::new(),
                delay: 0.0,
            },
            4 => Self::RepeatingInterval {
                interval: 1.0,
                max_count: None,
            },
            5 => Self::OnSpawn,
            6 => Self::AfterIdleTimeout { timeout: 2.0 },
            _ => Self::AtTime(0.0),
        }
    }

    pub const VARIANT_LABELS: &[&str] = &[
        "At Time",
        "On Collision",
        "On Effect Event",
        "After Rule",
        "Repeating Interval",
        "On Spawn",
        "After Idle Timeout",
    ];
}

// ---------------------------------------------------------------------------
// Actions
// ---------------------------------------------------------------------------

/// An action that executes when a step's trigger fires.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub enum EffectAction {
    /// Spawn a primitive entity as a child of the effect.
    SpawnPrimitive {
        tag: String,
        shape: PrimitiveShape,
        offset: Vec3,
        material: Option<String>,
        rigid_body: Option<RigidBodyKind>,
    },
    /// Spawn a particle effect (from particle library preset).
    SpawnParticle {
        tag: String,
        preset: String,
        at: SpawnLocation,
    },
    /// Set linear velocity on a tagged entity.
    SetVelocity { tag: String, velocity: Vec3 },
    /// Apply a one-shot impulse to a tagged entity.
    ApplyImpulse { tag: String, impulse: Vec3 },
    /// Remove a previously spawned entity.
    Despawn { tag: String },
    /// Emit a named event (triggers OnEffectEvent steps).
    EmitEvent(String),
    /// Enable/disable gravity on a tagged entity.
    SetGravity { tag: String, enabled: bool },
    /// Spawn a clustered decal at a location.
    SpawnDecal {
        tag: String,
        texture_path: String,
        at: SpawnLocation,
        scale: Vec3,
    },
    /// Spawn a GLTF/GLB model as a child of the effect.
    SpawnGltf {
        tag: String,
        path: String,
        at: SpawnLocation,
        scale: Vec3,
        rigid_body: Option<RigidBodyKind>,
    },
    /// Spawn a child effect from the effect library.
    SpawnEffect {
        tag: String,
        preset: String,
        at: SpawnLocation,
        inherit_velocity: bool,
    },
    /// Insert a component on a tagged entity via reflection.
    InsertComponent {
        target_tag: String,
        component_type: String,
        field_values: HashMap<String, String>,
    },
    /// Remove a component from a tagged entity via reflection.
    RemoveComponent {
        target_tag: String,
        component_type: String,
    },
    /// Animate a property over time with easing.
    TweenValue {
        target_tag: String,
        property: TweenProperty,
        from: f32,
        to: f32,
        duration: f32,
        easing: EasingType,
    },
}

impl EffectAction {
    pub fn label(&self) -> &'static str {
        match self {
            Self::SpawnPrimitive { .. } => "Spawn Primitive",
            Self::SpawnParticle { .. } => "Spawn Particle",
            Self::SetVelocity { .. } => "Set Velocity",
            Self::ApplyImpulse { .. } => "Apply Impulse",
            Self::Despawn { .. } => "Despawn",
            Self::EmitEvent(_) => "Emit Event",
            Self::SetGravity { .. } => "Set Gravity",
            Self::SpawnDecal { .. } => "Spawn Decal",
            Self::SpawnGltf { .. } => "Spawn GLTF",
            Self::SpawnEffect { .. } => "Spawn Effect",
            Self::InsertComponent { .. } => "Insert Component",
            Self::RemoveComponent { .. } => "Remove Component",
            Self::TweenValue { .. } => "Tween Value",
        }
    }

    pub fn variant_index(&self) -> usize {
        match self {
            Self::SpawnPrimitive { .. } => 0,
            Self::SpawnParticle { .. } => 1,
            Self::SetVelocity { .. } => 2,
            Self::ApplyImpulse { .. } => 3,
            Self::Despawn { .. } => 4,
            Self::EmitEvent(_) => 5,
            Self::SetGravity { .. } => 6,
            Self::SpawnDecal { .. } => 7,
            Self::SpawnGltf { .. } => 8,
            Self::SpawnEffect { .. } => 9,
            Self::InsertComponent { .. } => 10,
            Self::RemoveComponent { .. } => 11,
            Self::TweenValue { .. } => 12,
        }
    }

    pub const VARIANT_LABELS: &[&str] = &[
        "Spawn Primitive",
        "Spawn Particle",
        "Set Velocity",
        "Apply Impulse",
        "Despawn",
        "Emit Event",
        "Set Gravity",
        "Spawn Decal",
        "Spawn GLTF",
        "Spawn Effect",
        "Insert Component",
        "Remove Component",
        "Tween Value",
    ];

    pub fn from_variant_index(idx: usize) -> Self {
        match idx {
            0 => Self::SpawnPrimitive {
                tag: String::new(),
                shape: PrimitiveShape::Cube,
                offset: Vec3::ZERO,
                material: None,
                rigid_body: None,
            },
            1 => Self::SpawnParticle {
                tag: String::new(),
                preset: String::new(),
                at: SpawnLocation::Offset(Vec3::ZERO),
            },
            2 => Self::SetVelocity {
                tag: String::new(),
                velocity: Vec3::ZERO,
            },
            3 => Self::ApplyImpulse {
                tag: String::new(),
                impulse: Vec3::ZERO,
            },
            4 => Self::Despawn {
                tag: String::new(),
            },
            5 => Self::EmitEvent(String::new()),
            6 => Self::SetGravity {
                tag: String::new(),
                enabled: true,
            },
            7 => Self::SpawnDecal {
                tag: String::new(),
                texture_path: String::new(),
                at: SpawnLocation::Offset(Vec3::ZERO),
                scale: Vec3::ONE,
            },
            8 => Self::SpawnGltf {
                tag: String::new(),
                path: String::new(),
                at: SpawnLocation::Offset(Vec3::ZERO),
                scale: Vec3::ONE,
                rigid_body: None,
            },
            9 => Self::SpawnEffect {
                tag: String::new(),
                preset: String::new(),
                at: SpawnLocation::Offset(Vec3::ZERO),
                inherit_velocity: false,
            },
            10 => Self::InsertComponent {
                target_tag: String::new(),
                component_type: String::new(),
                field_values: HashMap::new(),
            },
            11 => Self::RemoveComponent {
                target_tag: String::new(),
                component_type: String::new(),
            },
            12 => Self::TweenValue {
                target_tag: String::new(),
                property: TweenProperty::Scale,
                from: 1.0,
                to: 0.0,
                duration: 1.0,
                easing: EasingType::Linear,
            },
            _ => Self::EmitEvent(String::new()),
        }
    }
}

/// Serializable rigid body kind (mirrors avian3d::RigidBody without the
/// non-serializable internals).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum RigidBodyKind {
    #[default]
    Dynamic,
    Static,
    Kinematic,
}

impl RigidBodyKind {
    pub fn to_rigid_body(self) -> RigidBody {
        match self {
            Self::Dynamic => RigidBody::Dynamic,
            Self::Static => RigidBody::Static,
            Self::Kinematic => RigidBody::Kinematic,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Dynamic => "Dynamic",
            Self::Static => "Static",
            Self::Kinematic => "Kinematic",
        }
    }

    pub const ALL: [Self; 3] = [Self::Dynamic, Self::Static, Self::Kinematic];
}

/// Where to spawn an entity relative to the effect.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub enum SpawnLocation {
    /// Relative to the effect entity.
    Offset(Vec3),
    /// At the collision point (only valid inside OnCollision steps).
    CollisionPoint,
}

impl Default for SpawnLocation {
    fn default() -> Self {
        Self::Offset(Vec3::ZERO)
    }
}

// ---------------------------------------------------------------------------
// Tween / animation types
// ---------------------------------------------------------------------------

/// Which property a tween animates.
#[derive(Serialize, Deserialize, Clone, Debug, Reflect)]
pub enum TweenProperty {
    Scale,
    Opacity,
    LightIntensity,
    Custom(String),
}

impl Default for TweenProperty {
    fn default() -> Self {
        Self::Scale
    }
}

impl TweenProperty {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Scale => "Scale",
            Self::Opacity => "Opacity",
            Self::LightIntensity => "Light Intensity",
            Self::Custom(_) => "Custom",
        }
    }

    pub fn variant_index(&self) -> usize {
        match self {
            Self::Scale => 0,
            Self::Opacity => 1,
            Self::LightIntensity => 2,
            Self::Custom(_) => 3,
        }
    }

    pub const VARIANT_LABELS: &[&str] = &["Scale", "Opacity", "Light Intensity", "Custom"];

    pub fn from_variant_index(idx: usize) -> Self {
        match idx {
            0 => Self::Scale,
            1 => Self::Opacity,
            2 => Self::LightIntensity,
            3 => Self::Custom(String::new()),
            _ => Self::Scale,
        }
    }
}

/// Easing function for tween animations.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum EasingType {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl EasingType {
    pub fn eval(&self, t: f32) -> f32 {
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => t * (2.0 - t),
            Self::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    -1.0 + (4.0 - 2.0 * t) * t
                }
            }
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Linear => "Linear",
            Self::EaseIn => "Ease In",
            Self::EaseOut => "Ease Out",
            Self::EaseInOut => "Ease In/Out",
        }
    }

    pub const ALL: [Self; 4] = [Self::Linear, Self::EaseIn, Self::EaseOut, Self::EaseInOut];
}

/// A currently-running tween animation (runtime only, not serialized).
pub struct ActiveTween {
    pub entity: Entity,
    pub property: TweenProperty,
    pub from: f32,
    pub to: f32,
    pub start_time: f32,
    pub duration: f32,
    pub easing: EasingType,
}

// ---------------------------------------------------------------------------
// Runtime playback state (NOT serialized)
// ---------------------------------------------------------------------------

/// Tracks effect playback state at runtime.
#[derive(Component)]
pub struct EffectPlayback {
    pub state: PlaybackState,
    pub elapsed: f32,
    /// Indices of steps already triggered.
    pub fired_steps: HashSet<usize>,
    /// Tag → live entity mapping for spawned children.
    pub spawned: HashMap<String, Entity>,
    /// Events emitted this frame (cleared each tick).
    pub pending_events: Vec<String>,
    /// Tags whose spawned entities collided this frame (cleared each tick).
    pub collision_tags: HashSet<String>,
    /// Last known collision point (for SpawnLocation::CollisionPoint).
    pub last_collision_point: Option<Vec3>,
    /// Rule name → elapsed time when that rule fired.
    pub rule_fire_times: HashMap<String, f32>,
    /// When any rule last fired (elapsed time).
    pub last_fire_time: f32,
    /// Running tween animations.
    pub active_tweens: Vec<ActiveTween>,
    /// Fire count per rule name (for repeating triggers).
    pub repeat_counts: HashMap<String, u32>,
}

impl Default for EffectPlayback {
    fn default() -> Self {
        Self {
            state: PlaybackState::Stopped,
            elapsed: 0.0,
            fired_steps: HashSet::new(),
            spawned: HashMap::new(),
            pending_events: Vec::new(),
            collision_tags: HashSet::new(),
            last_collision_point: None,
            rule_fire_times: HashMap::new(),
            last_fire_time: 0.0,
            active_tweens: Vec::new(),
            repeat_counts: HashMap::new(),
        }
    }
}

/// Current playback state of an effect.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PlaybackState {
    Playing,
    Paused,
    #[default]
    Stopped,
}

/// Marks entities spawned by an effect for cleanup.
#[derive(Component)]
pub struct EffectChild {
    pub effect_entity: Entity,
    pub tag: String,
}
