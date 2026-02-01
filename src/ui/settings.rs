use bevy::prelude::*;
use bevy_egui::{egui, EguiContext, EguiContextSettings, EguiContexts, EguiPrimaryContextPass};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use crate::editor::EditorState;

/// Resource to track if fonts have been loaded
#[derive(Resource, Default)]
struct FontsLoaded(bool);

/// Resource to track if font sizes need to be applied
#[derive(Resource, Default)]
struct FontSizesApplied(bool);

/// Font size settings for various UI elements
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FontSettings {
    /// Body text size (default labels, buttons)
    pub body: f32,
    /// Heading text size (section headers)
    pub heading: f32,
    /// Small text size (muted labels, status text)
    pub small: f32,
    /// Button text size
    pub button: f32,
    /// Monospace text size (code, values)
    pub monospace: f32,
}

impl Default for FontSettings {
    fn default() -> Self {
        Self {
            body: 14.0,
            heading: 16.0,
            small: 12.0,
            button: 14.0,
            monospace: 13.0,
        }
    }
}

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
    /// Maximum number of undo history entries
    #[serde(default = "default_undo_history_size")]
    pub undo_history_size: usize,
    /// Font size settings
    #[serde(default)]
    pub fonts: FontSettings,
    /// Show hotkey hints above status bar
    #[serde(default = "default_show_hints")]
    pub show_hints: bool,
}

fn default_show_hints() -> bool {
    true
}

fn default_undo_history_size() -> usize {
    50
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            ui_scale: 2.0,
            camera_speed: 10.0,
            camera_sensitivity: 0.003,
            grid_snap: 0.0,
            rotation_snap: 0.0,
            undo_history_size: 50,
            fonts: FontSettings::default(),
            show_hints: true,
        }
    }
}

impl Settings {
    /// Get the settings file path
    fn file_path() -> Option<PathBuf> {
        dirs::config_dir().map(|mut p| {
            p.push("bevy_modal_editor");
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
            .init_resource::<FontsLoaded>()
            .init_resource::<FontSizesApplied>()
            .add_systems(Startup, apply_settings_to_editor_state)
            .add_systems(Update, (apply_ui_scale, sync_snap_settings, load_custom_fonts, apply_font_sizes))
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
    mut editor_state: ResMut<EditorState>,
    mut font_sizes_applied: ResMut<FontSizesApplied>,
) -> Result {
    // Don't draw UI when editor is disabled
    if !editor_state.ui_enabled {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    egui::Window::new("Settings")
        .open(&mut window_state.open)
        .resizable(false)
        .show(ctx, |ui| {
            // UI Section
            ui.heading("Interface");
            egui::Grid::new("settings_ui_grid")
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

                    ui.label("Show Hints:");
                    if ui.checkbox(&mut settings.show_hints, "").changed() {
                        settings.save();
                    }
                    ui.end_row();
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // Font Sizes Section
            ui.heading("Font Sizes");
            egui::Grid::new("settings_fonts_grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Body:");
                    let response = ui.add(
                        egui::Slider::new(&mut settings.fonts.body, 10.0..=20.0)
                            .step_by(1.0)
                            .suffix("px"),
                    );
                    if response.changed() {
                        font_sizes_applied.0 = false;
                        settings.save();
                    }
                    ui.end_row();

                    ui.label("Heading:");
                    let response = ui.add(
                        egui::Slider::new(&mut settings.fonts.heading, 12.0..=24.0)
                            .step_by(1.0)
                            .suffix("px"),
                    );
                    if response.changed() {
                        font_sizes_applied.0 = false;
                        settings.save();
                    }
                    ui.end_row();

                    ui.label("Small:");
                    let response = ui.add(
                        egui::Slider::new(&mut settings.fonts.small, 8.0..=16.0)
                            .step_by(1.0)
                            .suffix("px"),
                    );
                    if response.changed() {
                        font_sizes_applied.0 = false;
                        settings.save();
                    }
                    ui.end_row();

                    ui.label("Button:");
                    let response = ui.add(
                        egui::Slider::new(&mut settings.fonts.button, 10.0..=20.0)
                            .step_by(1.0)
                            .suffix("px"),
                    );
                    if response.changed() {
                        font_sizes_applied.0 = false;
                        settings.save();
                    }
                    ui.end_row();

                    ui.label("Monospace:");
                    let response = ui.add(
                        egui::Slider::new(&mut settings.fonts.monospace, 10.0..=18.0)
                            .step_by(1.0)
                            .suffix("px"),
                    );
                    if response.changed() {
                        font_sizes_applied.0 = false;
                        settings.save();
                    }
                    ui.end_row();
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // Camera Section
            ui.heading("Camera");
            egui::Grid::new("settings_camera_grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Movement Speed:");
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

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // Snapping Section
            ui.heading("Snapping");
            egui::Grid::new("settings_snap_grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Grid Snap:");
                    let response = ui.add(
                        egui::Slider::new(&mut settings.grid_snap, 0.0..=2.0)
                            .step_by(0.25)
                            .custom_formatter(|v, _| {
                                if v == 0.0 {
                                    "Off".to_string()
                                } else {
                                    format!("{:.2}", v)
                                }
                            }),
                    );
                    if response.changed() {
                        editor_state.grid_snap = settings.grid_snap;
                        settings.save();
                    }
                    ui.end_row();

                    ui.label("Rotation Snap:");
                    let response = ui.add(
                        egui::Slider::new(&mut settings.rotation_snap, 0.0..=90.0)
                            .step_by(15.0)
                            .suffix("°")
                            .custom_formatter(|v, _| {
                                if v == 0.0 {
                                    "Off".to_string()
                                } else {
                                    format!("{:.0}°", v)
                                }
                            }),
                    );
                    if response.changed() {
                        editor_state.rotation_snap = settings.rotation_snap;
                        settings.save();
                    }
                    ui.end_row();
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            // History Section
            ui.heading("History");
            egui::Grid::new("settings_history_grid")
                .num_columns(2)
                .spacing([10.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Undo History Size:");
                    let response = ui.add(
                        egui::Slider::new(&mut settings.undo_history_size, 10..=200)
                            .step_by(10.0),
                    );
                    if response.changed() {
                        settings.save();
                    }
                    ui.end_row();
                });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            if ui.button("Reset to Defaults").clicked() {
                *settings = Settings::default();
                editor_state.grid_snap = settings.grid_snap;
                editor_state.rotation_snap = settings.rotation_snap;
                font_sizes_applied.0 = false;
                settings.save();
            }
        });

    Ok(())
}

/// Load custom Inter font for egui UI
fn load_custom_fonts(mut contexts: EguiContexts, mut fonts_loaded: ResMut<FontsLoaded>) {
    // Only load fonts once
    if fonts_loaded.0 {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    // Load the Inter font from assets
    let font_path = "assets/fonts/Inter-VariableFont_opsz,wght.ttf";
    let Ok(font_data) = fs::read(font_path) else {
        warn!("Failed to load font from {}", font_path);
        return;
    };

    let mut fonts = egui::FontDefinitions::default();

    // Add Inter font
    fonts.font_data.insert(
        "Inter".to_owned(),
        egui::FontData::from_owned(font_data).into(),
    );

    // Set Inter as the primary proportional font
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "Inter".to_owned());

    ctx.set_fonts(fonts);
    fonts_loaded.0 = true;
    info!("Loaded Inter font for UI");
}

/// Apply font sizes from settings to egui text styles
fn apply_font_sizes(
    mut contexts: EguiContexts,
    settings: Res<Settings>,
    mut font_sizes_applied: ResMut<FontSizesApplied>,
) {
    if font_sizes_applied.0 {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let mut style = (*ctx.style()).clone();

    // Update text styles with font sizes from settings
    style.text_styles.insert(
        egui::TextStyle::Body,
        egui::FontId::new(settings.fonts.body, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        egui::FontId::new(settings.fonts.heading, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Small,
        egui::FontId::new(settings.fonts.small, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        egui::FontId::new(settings.fonts.button, egui::FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Monospace,
        egui::FontId::new(settings.fonts.monospace, egui::FontFamily::Monospace),
    );

    ctx.set_style(style);
    font_sizes_applied.0 = true;
    info!("Applied font sizes from settings");
}
