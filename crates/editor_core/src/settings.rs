//! Editor settings (spec §7 "settings are data"): every user-tunable static value
//! lives in this one serde-ready resource. Defaults are code; a user
//! `editor-settings.ron` will layer over them exactly like the keymap file does.
//! `#[serde(default)]` at every level keeps old settings files forward-compatible.
//!
//! Design tokens that define the chrome's identity (spacing/radius scales, glyphs)
//! deliberately stay code constants — settings tune the editor, they don't fork the
//! design system.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Resource, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EditorSettings {
    pub camera: CameraSettings,
    pub viewport: ViewportSettings,
    pub ui: UiSettings,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CameraSettings {
    /// Fly-nav speed, world units per second.
    pub fly_speed: f32,
    /// Speed multiplier while Shift is held.
    pub fly_boost: f32,
    /// Mouse-look radians per pixel.
    pub look_sensitivity: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self { fly_speed: 10.0, fly_boost: 3.0, look_sensitivity: 0.0025 }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewportSettings {
    /// Selection outline color (linear RGBA).
    pub outline_color: [f32; 4],
    /// Selection outline width, pixels.
    pub outline_width: f32,
    /// Insert ghost tint (sRGBA).
    pub ghost_color: [f32; 4],
    /// Grid snap quantum, world units.
    pub grid_step: f32,
}

impl Default for ViewportSettings {
    fn default() -> Self {
        Self {
            outline_color: [0.35, 0.62, 1.0, 1.0],
            outline_width: 4.0,
            ghost_color: [0.35, 0.62, 1.0, 0.45],
            grid_step: 1.0,
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiSettings {
    /// Type scale (logical px): chips, badges, secondary metadata.
    pub font_size_xs: f32,
    /// Status bar, hints, binding glyphs, list metadata.
    pub font_size_s: f32,
    /// Body/list text.
    pub font_size_m: f32,
    /// Palette search input.
    pub font_size_search: f32,
    /// Transient status feedback (save/load flash) duration, seconds.
    pub status_flash_secs: f32,
    /// Unbound-key which-key popup auto-dismiss, seconds.
    pub which_key_dismiss_secs: f32,
    /// Hard cap on palette results (the list scrolls; guards pathological volume).
    pub palette_max_results: usize,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            font_size_xs: 11.0,
            font_size_s: 13.0,
            font_size_m: 14.0,
            font_size_search: 16.0,
            status_flash_secs: 3.0,
            which_key_dismiss_secs: 3.0,
            palette_max_results: 50,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The future editor-settings.ron contract: a PARTIAL file layers over defaults
    // (serde(default) at every level), and the full round-trip holds.
    #[test]
    fn partial_settings_file_layers_over_defaults() {
        let partial = r#"(camera: (fly_speed: 25.0), viewport: (outline_width: 2.0))"#;
        let s: EditorSettings = ron::from_str(partial).unwrap();
        assert_eq!(s.camera.fly_speed, 25.0);
        assert_eq!(s.camera.fly_boost, CameraSettings::default().fly_boost);
        assert_eq!(s.viewport.outline_width, 2.0);
        assert_eq!(s.ui.palette_max_results, UiSettings::default().palette_max_results);

        let text = ron::to_string(&EditorSettings::default()).unwrap();
        let round: EditorSettings = ron::from_str(&text).unwrap();
        assert_eq!(round.viewport.grid_step, EditorSettings::default().viewport.grid_step);
    }
}
