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
        }
    }

    pub fn variant_index(&self) -> usize {
        match self {
            Self::AtTime(_) => 0,
            Self::OnCollision { .. } => 1,
            Self::OnEffectEvent(_) => 2,
        }
    }

    pub fn from_variant_index(idx: usize) -> Self {
        match idx {
            0 => Self::AtTime(0.0),
            1 => Self::OnCollision {
                tag: String::new(),
            },
            2 => Self::OnEffectEvent(String::new()),
            _ => Self::AtTime(0.0),
        }
    }

    pub const VARIANT_LABELS: &[&str] = &["At Time", "On Collision", "On Effect Event"];
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
