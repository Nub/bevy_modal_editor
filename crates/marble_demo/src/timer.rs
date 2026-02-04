use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_modal_editor::SimulationState;

use crate::GoalZone;

use crate::marble::Marble;

/// Resource tracking the game timer
#[derive(Resource, Default)]
pub struct GameTimer {
    /// Elapsed time since play started
    pub elapsed: f32,
    /// Whether the level has been completed
    pub completed: bool,
    /// Completion time (set when goal is reached)
    pub completion_time: Option<f32>,
}

pub struct GameTimerPlugin;

impl Plugin for GameTimerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameTimer>()
            .add_systems(OnEnter(SimulationState::Playing), reset_timer)
            .add_systems(
                Update,
                (update_timer, check_goal_reached, draw_timer_ui)
                    .run_if(in_state(SimulationState::Playing)),
            );
    }
}

/// Reset timer when entering play mode
fn reset_timer(mut timer: ResMut<GameTimer>) {
    // Only reset if we're starting fresh (not resuming from pause)
    // The OnEnter trigger fires for both Editing->Playing and Paused->Playing
    // We want to keep the timer running when resuming from pause
    if timer.completed {
        // If completed, always reset for a new run
        *timer = GameTimer::default();
    }
    // If not completed, the timer continues from where it was
}

/// Update the elapsed timer
fn update_timer(time: Res<Time>, mut timer: ResMut<GameTimer>) {
    if !timer.completed {
        timer.elapsed += time.delta_secs();
    }
}

/// Check if the marble has reached the goal zone using collision detection
fn check_goal_reached(
    mut timer: ResMut<GameTimer>,
    marble_query: Query<&Transform, With<Marble>>,
    goal_query: Query<(&Transform, &Collider), With<GoalZone>>,
) {
    if timer.completed {
        return;
    }

    let Ok(marble_transform) = marble_query.single() else {
        return;
    };

    let marble_pos = marble_transform.translation;

    // Simple distance-based overlap check against goal zones
    for (goal_transform, _collider) in goal_query.iter() {
        let goal_pos = goal_transform.translation;
        let goal_half_extents = goal_transform.scale * 0.5;

        // AABB overlap check
        let diff = (marble_pos - goal_pos).abs();
        if diff.x < goal_half_extents.x + 0.5
            && diff.y < goal_half_extents.y + 0.5
            && diff.z < goal_half_extents.z + 0.5
        {
            timer.completed = true;
            timer.completion_time = Some(timer.elapsed);
            info!("Goal reached! Time: {:.2}s", timer.elapsed);
        }
    }
}

/// Draw a simple timer overlay UI
fn draw_timer_ui(
    _timer: Res<GameTimer>,
    _gizmos: Gizmos,
) {
    // Timer UI is drawn as screen-space text via the 2D overlay
    // For simplicity, we'll log completion — a full UI would use egui
    // The status bar already shows PLAYING state
}
