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
    /// Component type paths pinned to the top of the add-component palette.
    pub favorite_components: Vec<String>,
}

pub const SETTINGS_FILE: &str = "editor-settings.ron";

impl EditorSettings {
    /// Layer the user file over defaults (partial files work — serde defaults).
    pub fn load_user() -> Self {
        let mut settings: Self = std::fs::read_to_string(SETTINGS_FILE)
            .ok()
            .and_then(|text| ron::from_str(&text).ok())
            .unwrap_or_default();
        settings.migrate();
        settings
    }

    /// Settings are saved WHOLE, so every default a user ever ran with is
    /// frozen into their file — improving a default afterwards changes nothing
    /// for anyone who has already opened the editor once. This is the migrator
    /// for the cases where that froze something wrong.
    ///
    /// `palette_max_results` was 50, which is fewer than the editor's own
    /// action list: the command palette's first screen was cut partway through
    /// and a newcomer browsing it could not see that the rest existed. Nobody
    /// chose 50 — the editor wrote it — so it is migrated rather than
    /// respected.
    ///
    /// `dock_bottom_height` was 200, chosen when nothing was ever placed in the
    /// bottom dock — it is a tray, about five rows once the card chrome is
    /// paid for. Same reasoning: nobody picked it, the editor wrote it into an
    /// empty dock, and the asset browser is the first panel to live there.
    fn migrate(&mut self) {
        const OLD_PALETTE_CAP: usize = 50;
        const OLD_BOTTOM_HEIGHT: f32 = 200.0;
        if self.ui.palette_max_results == OLD_PALETTE_CAP {
            self.ui.palette_max_results = UiSettings::default().palette_max_results;
        }
        #[allow(clippy::float_cmp)]
        if self.ui.dock_bottom_height == OLD_BOTTOM_HEIGHT {
            self.ui.dock_bottom_height = UiSettings::default().dock_bottom_height;
        }
    }
    /// Persist the full settings (v1 discipline: saved on every change).
    pub fn save_user(&self) {
        if let Ok(text) = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()) {
            let _ = std::fs::write(SETTINGS_FILE, text);
        }
    }
    pub fn toggle_favorite_component(&mut self, type_path: &str) {
        if let Some(index) = self.favorite_components.iter().position(|p| p == type_path) {
            self.favorite_components.remove(index);
        } else {
            self.favorite_components.push(type_path.to_string());
        }
        self.save_user();
    }
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
    /// World units the viewport dollies per wheel notch.
    pub zoom_step: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            fly_speed: 10.0,
            fly_boost: 3.0,
            look_sensitivity: 0.0025,
            zoom_step: 0.9,
        }
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
    /// Angle snap quantum, DEGREES. 15° divides the turns a level is built
    /// from — 30, 45, 90 — so a wall meets a wall without anyone typing.
    pub angle_step: f32,
    /// How close two sockets must be to mate, metres. Measured SOCKET TO
    /// SOCKET, so it means what it says regardless of how big the piece is —
    /// the old origin-to-socket reach of 2m was unreachable for a piece bigger
    /// than itself, and meaningless for a small one.
    pub socket_reach: f32,
    /// Outline colour for a LOCKED selection (linear RGBA). The warn tone,
    /// not the selection blue: "selected" and "selected but frozen" are
    /// different states and must not look the same.
    pub locked_outline_color: [f32; 4],
}

impl Default for ViewportSettings {
    fn default() -> Self {
        Self {
            outline_color: [0.35, 0.62, 1.0, 1.0],
            outline_width: 4.0,
            ghost_color: [0.35, 0.62, 1.0, 0.45],
            grid_step: 1.0,
            angle_step: 15.0,
            socket_reach: 1.5,
            locked_outline_color: [0.82, 0.42, 0.16, 1.0],
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
    /// Hard cap on palette results (the list scrolls; guards pathological
    /// volume — a component palette can offer hundreds).
    ///
    /// It was 50, which is FEWER than the editor's own action list, so the
    /// command palette's first screen was cut partway through the alphabet and
    /// a newcomer browsing it could not see that the second half existed. The
    /// list scrolls; the cap is for the pathological case, not the normal one.
    pub palette_max_results: usize,
    /// Dock sizes, logical px (draggable resize arrives with the layout manager).
    pub dock_left_width: f32,
    pub dock_right_width: f32,
    pub dock_bottom_height: f32,
}

impl Default for UiSettings {
    fn default() -> Self {
        Self {
            font_size_xs: 11.0,
            font_size_s: 13.0,
            font_size_m: 14.0,
            font_size_search: 16.0,
            status_flash_secs: 3.0,
            palette_max_results: 200,
            dock_left_width: 280.0,
            dock_right_width: 320.0,
            dock_bottom_height: 260.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Settings are saved whole, so a value the editor itself wrote once is
    /// frozen forever — improving a default would otherwise change nothing for
    /// anyone who has already run the editor.
    #[test]
    fn the_old_palette_cap_is_migrated_not_respected() {
        let mut settings = EditorSettings::default();
        settings.ui.palette_max_results = 50;
        settings.migrate();
        assert_eq!(
            settings.ui.palette_max_results,
            UiSettings::default().palette_max_results,
            "a cap smaller than the action list is not a preference"
        );
    }

    /// A cap a user actually chose is left alone.
    #[test]
    fn a_deliberate_cap_survives() {
        let mut settings = EditorSettings::default();
        settings.ui.palette_max_results = 12;
        settings.migrate();
        assert_eq!(settings.ui.palette_max_results, 12);
    }

    /// Same reasoning for the bottom dock: 200 was written into a dock nothing
    /// was ever placed in, and it is about five rows once the card chrome is
    /// paid for. The asset browser is the first panel to live there.
    #[test]
    fn the_empty_dock_height_is_migrated_not_respected() {
        let mut settings = EditorSettings::default();
        settings.ui.dock_bottom_height = 200.0;
        settings.migrate();
        assert_eq!(
            settings.ui.dock_bottom_height,
            UiSettings::default().dock_bottom_height,
            "a height chosen for an empty dock is not a preference"
        );
        assert!(settings.ui.dock_bottom_height > 200.0);
    }

    /// A height the user dragged to is theirs.
    #[test]
    fn a_deliberate_dock_height_survives() {
        let mut settings = EditorSettings::default();
        settings.ui.dock_bottom_height = 340.0;
        settings.migrate();
        assert_eq!(settings.ui.dock_bottom_height, 340.0);
    }

    // The future editor-settings.ron contract: a PARTIAL file layers over defaults
    // (serde(default) at every level), and the full round-trip holds.
    #[test]
    fn partial_settings_file_layers_over_defaults() {
        let partial = r#"(camera: (fly_speed: 25.0), viewport: (outline_width: 2.0))"#;
        let s: EditorSettings = ron::from_str(partial).unwrap();
        assert_eq!(s.camera.fly_speed, 25.0);
        assert_eq!(s.camera.fly_boost, CameraSettings::default().fly_boost);
        assert_eq!(s.viewport.outline_width, 2.0);
        assert_eq!(
            s.ui.palette_max_results,
            UiSettings::default().palette_max_results
        );

        let text = ron::to_string(&EditorSettings::default()).unwrap();
        let round: EditorSettings = ron::from_str(&text).unwrap();
        assert_eq!(
            round.viewport.grid_step,
            EditorSettings::default().viewport.grid_step
        );
    }
}
