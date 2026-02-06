//! Game timer and HUD overlay using Bevy native UI.

use bevy::prelude::*;
use bevy::ui::px;
use bevy_editor_game::{GameEntity, GameStartedEvent, GameState};

use crate::coins::{AllCoinsCollectedEvent, CoinTracker};

/// Resource tracking the game timer.
#[derive(Resource, Default)]
pub struct GameTimer {
    /// Elapsed time since first movement.
    pub elapsed: f32,
    /// Whether the timer has started (first movement detected).
    pub started: bool,
    /// Whether all coins have been collected.
    pub completed: bool,
    /// Completion time (set when all coins collected).
    pub completion_time: Option<f32>,
}

// Marker components for HUD elements
#[derive(Component)]
struct TimerText;

#[derive(Component)]
struct CoinText;

#[derive(Component)]
struct CompletionOverlay;

#[derive(Component)]
struct CompletionTimeText;

pub struct GameTimerPlugin;

impl Plugin for GameTimerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameTimer>()
            .add_systems(Update, (reset_timer_on_game_start, spawn_hud_on_game_start))
            .add_systems(
                Update,
                (detect_first_movement, update_timer, handle_completion)
                    .chain()
                    .run_if(in_state(GameState::Playing)),
            )
            .add_systems(
                Update,
                (update_timer_text, update_coin_text, update_completion_overlay)
                    .run_if(in_state(GameState::Playing)),
            );
    }
}

/// Reset timer when the game starts.
fn reset_timer_on_game_start(
    mut events: MessageReader<GameStartedEvent>,
    mut timer: ResMut<GameTimer>,
) {
    for _ in events.read() {
        *timer = GameTimer::default();
    }
}

/// Spawn the HUD UI hierarchy when the game starts.
fn spawn_hud_on_game_start(
    mut events: MessageReader<GameStartedEvent>,
    mut commands: Commands,
) {
    for _ in events.read() {
        // Root container — full-screen overlay, no interaction
        commands
            .spawn((
                GameEntity,
                Name::new("HUD Root"),
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    position_type: PositionType::Absolute,
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                },
                // Don't block clicks
                Pickable::IGNORE,
            ))
            .with_children(|root| {
                // Top bar: timer + coin counter
                root.spawn((
                    Node {
                        margin: UiRect::top(px(12.0)),
                        padding: UiRect::axes(px(20.0), px(8.0)),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::all(px(8.0)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.6)),
                ))
                .with_children(|bar| {
                    // Timer text
                    bar.spawn((
                        TimerText,
                        Text::new("Move to start!"),
                        TextFont {
                            font_size: 24.0,
                            ..default()
                        },
                        TextColor(Color::WHITE),
                    ));

                    // Coin counter text
                    bar.spawn((
                        CoinText,
                        Text::new("0 / 0 coins"),
                        TextFont {
                            font_size: 16.0,
                            ..default()
                        },
                        TextColor(Color::srgb(1.0, 0.6, 0.0)),
                    ));
                });

                // Completion overlay — centered, hidden by default
                root.spawn((
                    CompletionOverlay,
                    Node {
                        flex_grow: 1.0,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        width: Val::Percent(100.0),
                        ..default()
                    },
                    Visibility::Hidden,
                ))
                .with_children(|overlay| {
                    overlay
                        .spawn((
                            Node {
                                padding: UiRect::axes(px(48.0), px(28.0)),
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Center,
                                row_gap: px(10.0),
                                border_radius: BorderRadius::all(px(12.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.8)),
                        ))
                        .with_children(|card| {
                            card.spawn((
                                Text::new("All Coins Collected!"),
                                TextFont {
                                    font_size: 32.0,
                                    ..default()
                                },
                                TextColor(Color::srgb(0.2, 1.0, 0.3)),
                            ));

                            card.spawn((
                                CompletionTimeText,
                                Text::new("Time: 0.00s"),
                                TextFont {
                                    font_size: 24.0,
                                    ..default()
                                },
                                TextColor(Color::WHITE),
                            ));

                            card.spawn((
                                Text::new("F7 to reset"),
                                TextFont {
                                    font_size: 14.0,
                                    ..default()
                                },
                                TextColor(Color::srgba(0.7, 0.7, 0.7, 1.0)),
                            ));
                        });
                });
            });
    }
}

/// Detect first movement input to start the timer.
fn detect_first_movement(keyboard: Res<ButtonInput<KeyCode>>, mut timer: ResMut<GameTimer>) {
    if timer.started {
        return;
    }

    let movement_keys = [
        KeyCode::KeyW,
        KeyCode::KeyA,
        KeyCode::KeyS,
        KeyCode::KeyD,
        KeyCode::Space,
    ];

    if movement_keys.iter().any(|k| keyboard.pressed(*k)) {
        timer.started = true;
        info!("Timer started!");
    }
}

/// Increment timer while active.
fn update_timer(time: Res<Time>, mut timer: ResMut<GameTimer>) {
    if timer.started && !timer.completed {
        timer.elapsed += time.delta_secs();
    }
}

/// Handle all-coins-collected event.
fn handle_completion(
    mut events: MessageReader<AllCoinsCollectedEvent>,
    mut timer: ResMut<GameTimer>,
) {
    for event in events.read() {
        timer.completed = true;
        timer.completion_time = Some(event.elapsed);
    }
}

/// Update the timer text display.
fn update_timer_text(
    timer: Res<GameTimer>,
    mut query: Query<(&mut Text, &mut TextColor), With<TimerText>>,
) {
    for (mut text, mut color) in query.iter_mut() {
        if timer.started {
            **text = format!("{:.2}s", timer.elapsed);
        } else {
            **text = "Move to start!".into();
        }

        color.0 = if timer.completed {
            Color::srgb(0.2, 1.0, 0.3)
        } else {
            Color::WHITE
        };
    }
}

/// Update the coin counter text.
fn update_coin_text(
    tracker: Res<CoinTracker>,
    mut query: Query<(&mut Text, &mut TextColor), With<CoinText>>,
) {
    for (mut text, mut color) in query.iter_mut() {
        **text = format!("{} / {} coins", tracker.collected, tracker.total);

        color.0 = if tracker.total > 0 && tracker.collected >= tracker.total {
            Color::srgb(0.2, 1.0, 0.3)
        } else {
            Color::srgb(1.0, 0.6, 0.0)
        };
    }
}

/// Show/hide completion overlay and update its time text.
fn update_completion_overlay(
    timer: Res<GameTimer>,
    mut overlays: Query<&mut Visibility, With<CompletionOverlay>>,
    mut time_texts: Query<&mut Text, With<CompletionTimeText>>,
) {
    for mut vis in overlays.iter_mut() {
        *vis = if timer.completed {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }

    if let Some(time) = timer.completion_time {
        for mut text in time_texts.iter_mut() {
            **text = format!("Time: {:.2}s", time);
        }
    }
}
