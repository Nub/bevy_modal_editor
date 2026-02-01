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
