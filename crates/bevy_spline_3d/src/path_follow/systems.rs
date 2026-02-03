use bevy::prelude::*;

use crate::geometry::CoordinateFrame;
use crate::spline::{approximate_arc_length, Spline};

use super::{FollowerEvent, FollowerEventKind, FollowerState, LoopMode, SplineFollower};

/// Number of samples for arc-length approximation.
const ARC_LENGTH_SAMPLES: usize = 128;

/// System that updates all spline followers.
pub fn update_spline_followers(
    mut followers: Query<(Entity, &mut SplineFollower, &mut Transform)>,
    splines: Query<(&Spline, &GlobalTransform)>,
    time: Res<Time>,
    mut events: MessageWriter<FollowerEvent>,
) {
    let delta = time.delta_secs();

    for (entity, mut follower, mut transform) in &mut followers {
        // Skip if not playing
        if follower.state != FollowerState::Playing {
            continue;
        }

        // Get the spline and its transform
        let Ok((spline, spline_transform)) = splines.get(follower.spline) else {
            continue;
        };

        if !spline.is_valid() {
            continue;
        }

        // Calculate t delta based on speed mode
        let dt = if follower.constant_speed {
            // Arc-length parameterization for constant speed
            let total_length = approximate_arc_length(spline, ARC_LENGTH_SAMPLES);
            if total_length > 0.0 {
                (follower.speed * delta) / total_length
            } else {
                0.0
            }
        } else {
            // Simple parametric - speed is in t units per second
            follower.speed * delta
        };

        // Update t based on direction
        let new_t = follower.t + dt * follower.direction;

        // Handle bounds and loop modes
        let (final_t, new_direction, event) = handle_bounds(new_t, follower.direction, follower.loop_mode);

        follower.t = final_t;
        follower.direction = new_direction;

        // Emit event if any
        if let Some(kind) = event {
            events.write(FollowerEvent { entity, kind });

            // Update state for finished
            if kind == FollowerEventKind::Finished {
                follower.state = FollowerState::Finished;
            }
        }

        // Update transform
        if let Some(local_position) = spline.evaluate(follower.t) {
            // Transform the local position to world space using the spline's transform
            let world_position = spline_transform.transform_point(local_position);

            let rotation = if follower.align_to_tangent {
                let local_rotation =
                    calculate_orientation(spline, follower.t, follower.up_vector, follower.direction);
                // Combine spline's rotation with the tangent-based rotation
                spline_transform.to_scale_rotation_translation().1 * local_rotation
            } else {
                transform.rotation
            };

            // Apply offset in local space (relative to follower's rotation)
            let world_offset = rotation * follower.offset;

            transform.translation = world_position + world_offset;
            transform.rotation = rotation;
        }
    }
}

/// Handle t value bounds based on loop mode.
/// Returns (new_t, new_direction, optional_event).
fn handle_bounds(
    t: f32,
    direction: f32,
    loop_mode: LoopMode,
) -> (f32, f32, Option<FollowerEventKind>) {
    match loop_mode {
        LoopMode::Once => {
            if t >= 1.0 {
                (1.0, direction, Some(FollowerEventKind::Finished))
            } else if t <= 0.0 {
                (0.0, direction, Some(FollowerEventKind::Finished))
            } else {
                (t, direction, None)
            }
        }
        LoopMode::Loop => {
            if t >= 1.0 {
                (t.fract(), direction, Some(FollowerEventKind::LoopCompleted))
            } else if t <= 0.0 {
                (1.0 + t.fract(), direction, Some(FollowerEventKind::LoopCompleted))
            } else {
                (t, direction, None)
            }
        }
        LoopMode::PingPong => {
            if t >= 1.0 {
                // Bounce back
                let overshoot = t - 1.0;
                (1.0 - overshoot, -1.0, Some(FollowerEventKind::ReachedEnd))
            } else if t <= 0.0 {
                // Bounce forward
                let overshoot = -t;
                (overshoot, 1.0, Some(FollowerEventKind::ReachedStart))
            } else {
                (t, direction, None)
            }
        }
    }
}

/// Calculate orientation from spline tangent.
fn calculate_orientation(spline: &Spline, t: f32, up: Vec3, direction: f32) -> Quat {
    let Some(tangent) = spline.evaluate_tangent(t) else {
        return Quat::IDENTITY;
    };

    let frame = CoordinateFrame::from_tangent_with_up(tangent, up);
    if !frame.is_valid() {
        return Quat::IDENTITY;
    }

    frame.to_rotation_with_direction(direction)
}

