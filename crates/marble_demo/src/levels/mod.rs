//! Level system: level registry, auto-generation, selection dialog, and completion flow.

pub mod level_gen;
mod the_descent;
mod the_gauntlet;
mod the_spiral;

use bevy::prelude::*;
use bevy_editor_game::GameState;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use bevy_modal_editor::scene::{LoadSceneEvent, SceneFile};
use bevy_modal_editor::ui::theme::{colors, draw_centered_dialog, DialogResult};
use bevy_modal_editor::SceneEntity;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Description of a single level.
#[derive(Clone)]
pub struct LevelInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub filename: &'static str,
    pub builder: fn(&mut World),
    pub index: usize,
}

/// Registry of all available levels.
#[derive(Resource, Clone)]
pub struct LevelRegistry {
    pub levels: Vec<LevelInfo>,
}

impl Default for LevelRegistry {
    fn default() -> Self {
        Self {
            levels: vec![
                LevelInfo {
                    name: "The Gauntlet",
                    description: "Multi-section obstacle course: corridors, pillars, bridge, and final ascent.",
                    filename: "the_gauntlet.scn.ron",
                    builder: the_gauntlet::build_the_gauntlet,
                    index: 0,
                },
                LevelInfo {
                    name: "The Spiral",
                    description: "Climb a vertical tower via spiraling ramps around a central column.",
                    filename: "the_spiral.scn.ron",
                    builder: the_spiral::build_the_spiral,
                    index: 1,
                },
                LevelInfo {
                    name: "The Descent",
                    description: "Start high and descend through platforms, bridges, and slalom pillars.",
                    filename: "the_descent.scn.ron",
                    builder: the_descent::build_the_descent,
                    index: 2,
                },
            ],
        }
    }
}

/// Tracks the current level and best completion times.
#[derive(Resource, Default)]
pub struct LevelState {
    pub current_level: usize,
    pub best_times: Vec<Option<f32>>,
}

/// Controls the level selection dialog.
#[derive(Resource, Default)]
pub struct LevelSelectDialog {
    pub open: bool,
}

/// Controls the level completion dialog.
#[derive(Resource, Default)]
pub struct LevelCompleteDialog {
    pub open: bool,
    pub completion_time: f32,
    pub level_index: usize,
}

/// Fired when the goal is reached during play.
#[derive(Message)]
pub struct LevelCompleteEvent {
    pub time: f32,
}

/// Pending multi-frame level action (Reset → Load → Play).
#[derive(Resource, Default)]
enum PendingLevelAction {
    #[default]
    None,
    /// Load a level by index on next frame (after reset completes).
    LoadLevel(usize),
    /// Load a level and auto-play.
    LoadAndPlay(usize),
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct LevelsPlugin;

impl Plugin for LevelsPlugin {
    fn build(&self, app: &mut App) {
        let registry = LevelRegistry::default();
        let num_levels = registry.levels.len();

        app.insert_resource(registry)
            .insert_resource(LevelState {
                current_level: 0,
                best_times: vec![None; num_levels],
            })
            .init_resource::<LevelSelectDialog>()
            .init_resource::<LevelCompleteDialog>()
            .init_resource::<PendingLevelAction>()
            .add_message::<LevelCompleteEvent>()
            .add_systems(
                PreStartup,
                level_gen::generate_missing_level_files,
            )
            .add_systems(Update, auto_open_level_select)
            .add_systems(Update, on_level_complete)
            .add_systems(Update, process_pending_action)
            .add_systems(
                EguiPrimaryContextPass,
                (draw_level_select, draw_level_complete),
            );
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Auto-open the level select dialog on startup if no scene is loaded.
fn auto_open_level_select(
    mut ran: Local<bool>,
    mut dialog: ResMut<LevelSelectDialog>,
    existing: Query<Entity, With<SceneEntity>>,
    scene_file: Res<SceneFile>,
) {
    if *ran {
        return;
    }
    *ran = true;

    // Only auto-open if no scene entities exist and no file is loaded
    if existing.iter().count() == 0 && scene_file.path.is_none() {
        dialog.open = true;
    }
}

/// Draw the level selection dialog.
fn draw_level_select(
    mut contexts: EguiContexts,
    mut dialog: ResMut<LevelSelectDialog>,
    registry: Res<LevelRegistry>,
    mut level_state: ResMut<LevelState>,
    mut load_events: MessageWriter<LoadSceneEvent>,
    mut scene_file: ResMut<SceneFile>,
) -> Result {
    if !dialog.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let result = draw_centered_dialog(ctx, "Level Select", [450.0, 380.0], |ui| {
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Choose a level to load into the editor")
                .color(colors::TEXT_SECONDARY),
        );
        ui.add_space(12.0);

        let mut selected = None;

        for level in &registry.levels {
            let is_current = level.index == level_state.current_level;

            let frame = egui::Frame::NONE
                .fill(if is_current {
                    colors::SELECTION_BG
                } else {
                    colors::BG_MEDIUM
                })
                .corner_radius(egui::CornerRadius::same(6))
                .inner_margin(egui::Margin::same(10));

            let response = frame
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}. {}",
                                    level.index + 1,
                                    level.name,
                                ))
                                .color(colors::TEXT_PRIMARY)
                                .strong(),
                            );
                            ui.label(
                                egui::RichText::new(level.description)
                                    .color(colors::TEXT_SECONDARY)
                                    .small(),
                            );
                            if let Some(Some(time)) = level_state.best_times.get(level.index) {
                                ui.label(
                                    egui::RichText::new(format!("Best: {:.2}s", time))
                                        .color(colors::ACCENT_GREEN)
                                        .small(),
                                );
                            }
                        });
                    });
                })
                .response
                .interact(egui::Sense::click());

            if response.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }

            if response.clicked() {
                selected = Some(level.index);
            }

            ui.add_space(4.0);
        }

        if let Some(idx) = selected {
            level_state.current_level = idx;
            let path = level_gen::level_path(&registry.levels[idx].filename);
            load_events.write(LoadSceneEvent { path: path.clone() });
            // Update SceneFile so the window title reflects the level
            scene_file.path = Some(path);
            scene_file.clear_modified();
            return DialogResult::Confirmed;
        }

        DialogResult::None
    });

    match result {
        DialogResult::Confirmed | DialogResult::Close => {
            dialog.open = false;
        }
        DialogResult::None => {}
    }

    Ok(())
}

/// React to level completion events.
fn on_level_complete(
    mut events: MessageReader<LevelCompleteEvent>,
    mut complete_dialog: ResMut<LevelCompleteDialog>,
    mut level_state: ResMut<LevelState>,
) {
    for event in events.read() {
        let idx = level_state.current_level;
        complete_dialog.open = true;
        complete_dialog.completion_time = event.time;
        complete_dialog.level_index = idx;

        // Update best time
        if let Some(slot) = level_state.best_times.get_mut(idx) {
            match slot {
                Some(best) if event.time < *best => *slot = Some(event.time),
                None => *slot = Some(event.time),
                _ => {}
            }
        }
    }
}

/// Draw the level completion dialog.
fn draw_level_complete(
    mut contexts: EguiContexts,
    mut complete_dialog: ResMut<LevelCompleteDialog>,
    registry: Res<LevelRegistry>,
    mut reset_events: MessageWriter<bevy_editor_game::ResetEvent>,
    mut pending: ResMut<PendingLevelAction>,
    game_state: Res<State<GameState>>,
) -> Result {
    if !complete_dialog.open {
        return Ok(());
    }

    // Only show during Playing state
    if *game_state.get() != GameState::Playing {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let level_name = registry
        .levels
        .get(complete_dialog.level_index)
        .map(|l| l.name)
        .unwrap_or("Unknown");
    let has_next = complete_dialog.level_index + 1 < registry.levels.len();

    let mut action = None;

    egui::Window::new("Level Complete!")
        .collapsible(false)
        .resizable(false)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(colors::BG_DARK)
                .shadow(bevy_modal_editor::ui::theme::WINDOW_SHADOW),
        )
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([350.0, 220.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!("{} completed!", level_name))
                        .color(colors::ACCENT_GREEN)
                        .heading(),
                );
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(format!("Time: {:.2}s", complete_dialog.completion_time))
                        .color(colors::TEXT_PRIMARY)
                        .size(18.0),
                );
                ui.add_space(16.0);

                ui.horizontal(|ui| {
                    ui.add_space(20.0);

                    if has_next && ui.button("Next Level").clicked() {
                        action = Some("next");
                    }

                    if ui.button("Replay").clicked() {
                        action = Some("replay");
                    }

                    if ui.button("Level Select").clicked() {
                        action = Some("select");
                    }
                });
            });
        });

    if let Some(act) = action {
        complete_dialog.open = false;
        match act {
            "next" => {
                let next_idx = complete_dialog.level_index + 1;
                *pending = PendingLevelAction::LoadAndPlay(next_idx);
                reset_events.write(bevy_editor_game::ResetEvent);
            }
            "replay" => {
                let idx = complete_dialog.level_index;
                *pending = PendingLevelAction::LoadAndPlay(idx);
                reset_events.write(bevy_editor_game::ResetEvent);
            }
            "select" => {
                *pending = PendingLevelAction::None;
                reset_events.write(bevy_editor_game::ResetEvent);
                // Will open select dialog after reset
                // (handled in process_pending_action via separate path)
            }
            _ => {}
        }

        // For "select", we set a special pending action
        if act == "select" {
            *pending = PendingLevelAction::LoadLevel(usize::MAX); // sentinel for "open select"
        }
    }

    Ok(())
}

/// Process pending level actions after a reset completes.
fn process_pending_action(
    mut pending: ResMut<PendingLevelAction>,
    game_state: Res<State<GameState>>,
    registry: Res<LevelRegistry>,
    mut load_events: MessageWriter<LoadSceneEvent>,
    mut play_events: MessageWriter<bevy_editor_game::PlayEvent>,
    mut level_state: ResMut<LevelState>,
    mut select_dialog: ResMut<LevelSelectDialog>,
    mut scene_file: ResMut<SceneFile>,
) {
    // Only process when we're back in Editing state (reset completed)
    if *game_state.get() != GameState::Editing {
        return;
    }

    match *pending {
        PendingLevelAction::None => {}
        PendingLevelAction::LoadLevel(idx) => {
            *pending = PendingLevelAction::None;
            if idx == usize::MAX {
                // Sentinel: open level select dialog
                select_dialog.open = true;
            } else if let Some(level) = registry.levels.get(idx) {
                level_state.current_level = idx;
                let path = level_gen::level_path(level.filename);
                load_events.write(LoadSceneEvent { path: path.clone() });
                scene_file.path = Some(path);
                scene_file.clear_modified();
            }
        }
        PendingLevelAction::LoadAndPlay(idx) => {
            *pending = PendingLevelAction::None;
            if let Some(level) = registry.levels.get(idx) {
                level_state.current_level = idx;
                let path = level_gen::level_path(level.filename);
                load_events.write(LoadSceneEvent { path: path.clone() });
                scene_file.path = Some(path);
                scene_file.clear_modified();
                play_events.write(bevy_editor_game::PlayEvent);
            }
        }
    }
}
