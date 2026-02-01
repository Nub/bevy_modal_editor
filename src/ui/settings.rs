use bevy::prelude::*;
use bevy_egui::{egui, EguiContext, EguiContextSettings, EguiContexts, EguiPrimaryContextPass};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::editor::EditorState;

/// Application settings that persist to disk
#[derive(Resource, Serialize, Deserialize, Clone)]
pub struct Settings {
    /// UI scale factor (1.0 = default)
    pub ui_scale: f32,
    /// Camera movement speed
    pub camera_speed: f32,
    /// Camera mouse sensitivity
    pub camera_sensitivity: f32,
    /// Grid snap amount (0.0 = disabled)
    #[serde(default)]
    pub grid_snap: f32,
    /// Rotation snap in degrees (0.0 = disabled)
    #[serde(default)]
    pub rotation_snap: f32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: 1.5,
            camera_speed: 10.0,
            camera_sensitivity: 0.003,
            grid_snap: 0.0,
            rotation_snap: 0.0,
        }
    }
}

impl Settings {
    /// Get the settings file path
    fn file_path() -> Option<PathBuf> {
        dirs::config_dir().map(|mut p| {
            p.push("bevy_avian3d_editor");
            p.push("settings.ron");
            p
        })
    }

    /// Load settings from disk, or return defaults if not found
    pub fn load() -> Self {
        let Some(path) = Self::file_path() else {
            return Self::default();
        };

        match fs::read_to_string(&path) {
            Ok(content) => ron::from_str(&content).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Save settings to disk
    pub fn save(&self) {
        let Some(path) = Self::file_path() else {
            error!("Could not determine config directory");
            return;
        };

        // Create parent directories if needed
        if let Some(parent) = path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                error!("Failed to create config directory: {}", e);
                return;
            }
        }

        match ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()) {
            Ok(content) => {
                if let Err(e) = fs::write(&path, content) {
                    error!("Failed to save settings: {}", e);
                } else {
                    info!("Settings saved to: {:?}", path);
                }
            }
            Err(e) => {
                error!("Failed to serialize settings: {}", e);
            }
        }
    }
}

/// Resource to track if settings window is open
#[derive(Resource, Default)]
pub struct SettingsWindowState {
    pub open: bool,
}

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        // Load settings on startup
        let settings = Settings::load();
        app.insert_resource(settings)
            .init_resource::<SettingsWindowState>()
            .add_systems(Startup, apply_settings_to_editor_state)
            .add_systems(Update, (apply_ui_scale, sync_snap_settings))
            .add_systems(EguiPrimaryContextPass, draw_settings_window);
    }
}

/// Apply loaded settings to EditorState on startup
fn apply_settings_to_editor_state(settings: Res<Settings>, mut editor_state: ResMut<EditorState>) {
    editor_state.grid_snap = settings.grid_snap;
    editor_state.rotation_snap = settings.rotation_snap;
}

/// Sync snap settings from EditorState to Settings and save when changed
fn sync_snap_settings(editor_state: Res<EditorState>, mut settings: ResMut<Settings>) {
    if editor_state.grid_snap != settings.grid_snap
        || editor_state.rotation_snap != settings.rotation_snap
    {
        settings.grid_snap = editor_state.grid_snap;
        settings.rotation_snap = editor_state.rotation_snap;
        settings.save();
    }
}

/// Apply UI scale to egui
fn apply_ui_scale(
    settings: Res<Settings>,
    mut query: Query<&mut EguiContextSettings, With<EguiContext>>,
) {
    for mut ctx_settings in &mut query {
        ctx_settings.scale_factor = settings.ui_scale;
    }
}

/// Draw the settings window
fn draw_settings_window(
    mut contexts: EguiContexts,
    mut settings: ResMut<Settings>,
    mut window_state: ResMut<SettingsWindowState>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::Window::new("Settings")
        .open(&mut window_state.open)
        .resizable(false)
        .show(ctx, |ui| {
            egui::Grid::new("settings_grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("UI Scale:");
                    let response = ui.add(
                        egui::Slider::new(&mut settings.ui_scale, 0.75..=3.0)
                            .step_by(0.25)
                            .suffix("x"),
                    );
                    if response.changed() {
                        settings.save();
                    }
                    ui.end_row();

                    ui.label("Camera Speed:");
                    let response = ui.add(
                        egui::Slider::new(&mut settings.camera_speed, 1.0..=50.0)
                            .step_by(1.0),
                    );
                    if response.changed() {
                        settings.save();
                    }
                    ui.end_row();

                    ui.label("Mouse Sensitivity:");
                    let response = ui.add(
                        egui::Slider::new(&mut settings.camera_sensitivity, 0.001..=0.01)
                            .step_by(0.001),
                    );
                    if response.changed() {
                        settings.save();
                    }
                    ui.end_row();
                });

            ui.separator();

            if ui.button("Reset to Defaults").clicked() {
                *settings = Settings::default();
                settings.save();
            }
        });

    Ok(())
}
