use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

/// Resource to track if theme has been applied
#[derive(Resource, Default)]
pub struct ThemeApplied(pub bool);

pub struct ThemePlugin;

impl Plugin for ThemePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ThemeApplied>()
            .add_systems(Update, apply_dark_theme);
    }
}

/// Standard shadow for floating windows and popups
pub const WINDOW_SHADOW: egui::Shadow = egui::Shadow {
    offset: [0, 2],
    blur: 8,
    spread: 0,
    color: egui::Color32::from_black_alpha(60),
};

/// Lighter shadow for side panels
pub const PANEL_SHADOW: egui::Shadow = egui::Shadow {
    offset: [0, 2],
    blur: 4,
    spread: 0,
    color: egui::Color32::from_black_alpha(40),
};

/// Shared configuration for side panels (Inspector, Material, Hierarchy)
pub mod panel {
    /// Padding from window edges
    pub const WINDOW_PADDING: f32 = 8.0;
    /// Height of the status bar at bottom
    pub const STATUS_BAR_HEIGHT: f32 = 24.0;
    /// Default panel width
    pub const DEFAULT_WIDTH: f32 = 250.0;
    /// Minimum panel width
    pub const MIN_WIDTH: f32 = 250.0;
    /// Minimum panel height
    pub const MIN_HEIGHT: f32 = 100.0;
    /// Title bar height for content calculations
    pub const TITLE_BAR_HEIGHT: f32 = 28.0;
    /// Bottom padding inside panel
    pub const BOTTOM_PADDING: f32 = 30.0;
}

/// Create a standard window frame with consistent styling
pub fn window_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style)
        .fill(colors::BG_DARK)
        .shadow(WINDOW_SHADOW)
}

/// Create a side panel frame with consistent styling
pub fn panel_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::window(style)
        .fill(colors::PANEL_BG)
        .shadow(PANEL_SHADOW)
}

/// Create a popup/tooltip frame with consistent styling
pub fn popup_frame(style: &egui::Style) -> egui::Frame {
    egui::Frame::popup(style)
        .fill(colors::BG_DARK.gamma_multiply(0.95))
        .corner_radius(egui::CornerRadius::same(8))
        .shadow(WINDOW_SHADOW)
}

/// Result of a dialog draw operation
pub enum DialogResult {
    /// Dialog remains open, no action taken
    None,
    /// Dialog should be closed
    Close,
    /// Dialog confirmed with an action
    Confirmed,
}

/// Draw a centered modal dialog window.
///
/// This provides consistent styling for modal dialogs including:
/// - Centered positioning
/// - Standard frame styling
/// - ESC key handling for closing
///
/// # Arguments
/// * `ctx` - The egui context
/// * `title` - Window title
/// * `size` - Fixed size of the dialog `[width, height]`
/// * `content` - Closure that draws the dialog content and returns a DialogResult
///
/// # Returns
/// The DialogResult from the content closure, or Close if ESC was pressed
pub fn draw_centered_dialog<F>(
    ctx: &egui::Context,
    title: &str,
    size: [f32; 2],
    content: F,
) -> DialogResult
where
    F: FnOnce(&mut egui::Ui) -> DialogResult,
{
    let mut result = DialogResult::None;

    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .frame(window_frame(&ctx.style()))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size(size)
        .show(ctx, |ui| {
            result = content(ui);
        });

    // Handle ESC to close
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        return DialogResult::Close;
    }

    result
}

/// Draw an error dialog with a title, message, and OK button.
///
/// # Returns
/// `true` if the dialog should be closed (OK clicked or ESC pressed)
pub fn draw_error_dialog(ctx: &egui::Context, title: &str, message: &str) -> bool {
    let mut should_close = false;

    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .frame(window_frame(&ctx.style()))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([400.0, 150.0])
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(10.0);
                ui.label(egui::RichText::new(message).color(colors::TEXT_PRIMARY));
                ui.add_space(20.0);
                if ui.button("OK").clicked() {
                    should_close = true;
                }
            });
        });

    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        should_close = true;
    }

    should_close
}

/// Color palette matching Bevy editor style
pub mod colors {
    use bevy_egui::egui::Color32;

    // Backgrounds (with slight transparency for floating windows)
    pub const BG_DARKEST: Color32 = Color32::from_rgb(20, 20, 22);
    pub const BG_DARK: Color32 = Color32::from_rgba_premultiplied(25, 25, 28, 250);
    pub const BG_MEDIUM: Color32 = Color32::from_rgb(40, 40, 43);
    pub const BG_LIGHT: Color32 = Color32::from_rgb(50, 50, 53);
    pub const BG_HIGHLIGHT: Color32 = Color32::from_rgb(60, 60, 65);

    // Panel colors
    pub const PANEL_BG: Color32 = Color32::from_rgba_premultiplied(25, 25, 28, 250);
    pub const PANEL_HEADER: Color32 = Color32::from_rgb(40, 40, 43);

    // Text colors
    pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(220, 220, 220);
    pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(160, 160, 160);
    pub const TEXT_MUTED: Color32 = Color32::from_rgb(120, 120, 120);

    // Accent colors
    pub const ACCENT_BLUE: Color32 = Color32::from_rgb(86, 156, 214);
    pub const ACCENT_GREEN: Color32 = Color32::from_rgb(78, 201, 176);
    pub const ACCENT_ORANGE: Color32 = Color32::from_rgb(206, 145, 87);
    pub const ACCENT_PURPLE: Color32 = Color32::from_rgb(180, 142, 210);
    pub const ACCENT_CYAN: Color32 = Color32::from_rgb(78, 201, 214);

    // Selection/highlight
    pub const SELECTION_BG: Color32 = Color32::from_rgb(38, 79, 120);
    pub const HOVER_BG: Color32 = Color32::from_rgb(50, 50, 55);

    // Axis colors (for transform gizmos/labels)
    pub const AXIS_X: Color32 = Color32::from_rgb(230, 90, 90);
    pub const AXIS_Y: Color32 = Color32::from_rgb(90, 200, 90);
    pub const AXIS_Z: Color32 = Color32::from_rgb(90, 140, 230);

    // Widget colors
    pub const WIDGET_BG: Color32 = Color32::from_rgb(50, 50, 53);
    pub const WIDGET_BG_HOVER: Color32 = Color32::from_rgb(60, 60, 65);
    pub const WIDGET_BG_ACTIVE: Color32 = Color32::from_rgb(70, 70, 75);
    pub const WIDGET_BORDER: Color32 = Color32::from_rgb(70, 70, 75);

    // Status colors
    pub const STATUS_SUCCESS: Color32 = Color32::from_rgb(80, 200, 120);
    pub const STATUS_WARNING: Color32 = Color32::from_rgb(230, 180, 80);
    pub const STATUS_ERROR: Color32 = Color32::from_rgb(230, 90, 90);
}

/// Apply the dark theme to egui
fn apply_dark_theme(mut contexts: EguiContexts, mut theme_applied: ResMut<ThemeApplied>) {
    if theme_applied.0 {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

    let mut style = (*ctx.style()).clone();

    // Spacing
    style.spacing.item_spacing = egui::vec2(8.0, 4.0);
    style.spacing.window_margin = egui::Margin::same(8);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.indent = 18.0;

    // Rounding (corner radius)
    style.visuals.window_corner_radius = egui::CornerRadius::same(6);
    style.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(4);
    style.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
    style.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
    style.visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);

    // Window styling
    style.visuals.window_fill = colors::BG_DARK;
    style.visuals.window_stroke = egui::Stroke::new(1.0, colors::WIDGET_BORDER);
    style.visuals.panel_fill = colors::PANEL_BG;

    // Widget backgrounds
    style.visuals.widgets.noninteractive.bg_fill = colors::WIDGET_BG;
    style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, colors::TEXT_SECONDARY);
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, colors::WIDGET_BORDER);

    style.visuals.widgets.inactive.bg_fill = colors::WIDGET_BG;
    style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, colors::TEXT_PRIMARY);
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, colors::WIDGET_BORDER);

    style.visuals.widgets.hovered.bg_fill = colors::WIDGET_BG_HOVER;
    style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, colors::TEXT_PRIMARY);
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, colors::ACCENT_BLUE);

    style.visuals.widgets.active.bg_fill = colors::WIDGET_BG_ACTIVE;
    style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, colors::TEXT_PRIMARY);
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, colors::ACCENT_BLUE);

    // Selection colors
    style.visuals.selection.bg_fill = colors::SELECTION_BG;
    style.visuals.selection.stroke = egui::Stroke::new(1.0, colors::ACCENT_BLUE);

    // Hyperlink color
    style.visuals.hyperlink_color = colors::ACCENT_BLUE;

    // Extreme background (for text edit, etc)
    style.visuals.extreme_bg_color = colors::BG_DARKEST;

    // Faint bg color (for striped backgrounds)
    style.visuals.faint_bg_color = colors::BG_MEDIUM;

    // Text colors
    style.visuals.override_text_color = Some(colors::TEXT_PRIMARY);

    // Separator
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, colors::BG_LIGHT);

    ctx.set_style(style);
    theme_applied.0 = true;

    info!("Applied dark editor theme");
}
