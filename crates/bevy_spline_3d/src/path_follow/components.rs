use bevy::prelude::*;

/// How the follower behaves when reaching the end of the spline.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Reflect)]
#[reflect(Default)]
pub enum LoopMode {
    /// Stop at the end of the spline.
    #[default]
    Once,
    /// Loop back to the start when reaching the end.
    Loop,
    /// Reverse direction at each end (ping-pong).
    PingPong,
}

/// Current state of a spline follower.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Reflect)]
#[reflect(Default)]
pub enum FollowerState {
    /// Follower is actively moving.
    #[default]
    Playing,
    /// Follower is paused.
    Paused,
    /// Follower has finished (only for LoopMode::Once).
    Finished,
}

/// Component that makes an entity follow a spline path.
///
/// The entity's [`Transform`] will be updated each frame to move along the spline
/// at the specified speed.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component, Default)]
pub struct SplineFollower {
    /// The spline entity to follow.
    pub spline: Entity,

    /// Movement speed in world units per second.
    pub speed: f32,

    /// Current parametric position on the spline (0.0 to 1.0).
    pub t: f32,

    /// How to handle reaching the end of the spline.
    pub loop_mode: LoopMode,

    /// Current playback state.
    pub state: FollowerState,

    /// Whether to align the entity's rotation to the spline tangent.
    ///
    /// When true, the entity's forward direction (-Z) will point along the spline.
    pub align_to_tangent: bool,

    /// Up vector used for orientation when `align_to_tangent` is true.
    pub up_vector: Vec3,

    /// Direction of travel: 1.0 for forward, -1.0 for backward.
    /// Used internally for ping-pong mode.
    pub direction: f32,

    /// Offset applied in local space relative to the spline position.
    pub offset: Vec3,

    /// Whether to use arc-length parameterization for constant speed.
    ///
    /// When true, the follower moves at a constant world-space speed.
    /// When false, speed varies based on control point density.
    pub constant_speed: bool,
}

impl Default for SplineFollower {
    fn default() -> Self {
        Self {
            spline: Entity::PLACEHOLDER,
            speed: 1.0,
            t: 0.0,
            loop_mode: LoopMode::Once,
            state: FollowerState::Playing,
            align_to_tangent: true,
            up_vector: Vec3::Y,
            direction: 1.0,
            offset: Vec3::ZERO,
            constant_speed: true,
        }
    }
}

impl SplineFollower {
    /// Create a new follower for the given spline.
    pub fn new(spline: Entity) -> Self {
        Self {
            spline,
            ..default()
        }
    }

    /// Set the movement speed.
    pub fn with_speed(mut self, speed: f32) -> Self {
        self.speed = speed;
        self
    }

    /// Set the starting position (0.0 to 1.0).
    pub fn with_start_t(mut self, t: f32) -> Self {
        self.t = t.clamp(0.0, 1.0);
        self
    }

    /// Set the loop mode.
    pub fn with_loop_mode(mut self, mode: LoopMode) -> Self {
        self.loop_mode = mode;
        self
    }

    /// Enable or disable tangent alignment.
    pub fn with_align_to_tangent(mut self, align: bool) -> Self {
        self.align_to_tangent = align;
        self
    }

    /// Set the up vector for orientation.
    pub fn with_up_vector(mut self, up: Vec3) -> Self {
        self.up_vector = up;
        self
    }

    /// Set a local-space offset from the spline position.
    pub fn with_offset(mut self, offset: Vec3) -> Self {
        self.offset = offset;
        self
    }

    /// Enable or disable constant speed (arc-length parameterization).
    pub fn with_constant_speed(mut self, constant: bool) -> Self {
        self.constant_speed = constant;
        self
    }

    /// Start or resume playback.
    pub fn play(&mut self) {
        self.state = FollowerState::Playing;
    }

    /// Pause playback.
    pub fn pause(&mut self) {
        self.state = FollowerState::Paused;
    }

    /// Reset to the start of the spline.
    pub fn reset(&mut self) {
        self.t = 0.0;
        self.direction = 1.0;
        self.state = FollowerState::Playing;
    }

    /// Check if the follower has finished (only relevant for LoopMode::Once).
    pub fn is_finished(&self) -> bool {
        self.state == FollowerState::Finished
    }

    /// Check if the follower is currently playing.
    pub fn is_playing(&self) -> bool {
        self.state == FollowerState::Playing
    }
}

/// Message emitted when a follower reaches a significant point.
#[derive(Message, Debug, Clone)]
pub struct FollowerEvent {
    /// The entity with the SplineFollower component.
    pub entity: Entity,
    /// The type of event.
    pub kind: FollowerEventKind,
}

/// Types of follower events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FollowerEventKind {
    /// Follower reached the end of the spline.
    ReachedEnd,
    /// Follower reached the start of the spline (ping-pong mode).
    ReachedStart,
    /// Follower completed a full loop.
    LoopCompleted,
    /// Follower finished (LoopMode::Once).
    Finished,
}
