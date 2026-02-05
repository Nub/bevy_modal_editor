use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_editor_game::{GameStartedEvent, GameState};
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use bevy_modal_editor::ui::theme::colors;

use crate::levels::LevelCompleteEvent;
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
            .add_systems(Update, reset_timer_on_game_start)
            .add_systems(
                Update,
                (update_timer, check_goal_reached)
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(EguiPrimaryContextPass, draw_timer_ui.run_if(in_state(GameState::Playing)));
    }
}

/// Reset timer when the game starts fresh (from Editing)
fn reset_timer_on_game_start(
    mut events: MessageReader<GameStartedEvent>,
    mut timer: ResMut<GameTimer>,
) {
    for _ in events.read() {
        *timer = GameTimer::default();
    }
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
    mut complete_events: MessageWriter<LevelCompleteEvent>,
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
            complete_events.write(LevelCompleteEvent { time: timer.elapsed });
        }
    }
}

/// Draw the timer HUD overlay during play
fn draw_timer_ui(
    timer: Res<GameTimer>,
    mut contexts: EguiContexts,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::Area::new(egui::Id::new("timer_hud"))
        .anchor(egui::Align2::CENTER_TOP, [0.0, 8.0])
        .show(ctx, |ui| {
            let frame = egui::Frame::NONE
                .fill(egui::Color32::from_black_alpha(160))
                .corner_radius(egui::CornerRadius::same(8))
                .inner_margin(egui::Margin::symmetric(16, 6));

            frame.show(ui, |ui| {
                let time_text = format!("{:.2}s", timer.elapsed);
                let color = if timer.completed {
                    colors::ACCENT_GREEN
                } else {
                    colors::TEXT_PRIMARY
                };
                ui.label(
                    egui::RichText::new(time_text)
                        .color(color)
                        .size(20.0)
                        .monospace(),
                );
            });
        });

    Ok(())
}
