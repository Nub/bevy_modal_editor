//! The single source of editor-chrome styling (design bar, spec §7): every widget
//! draws its spacing, radii, colors, fonts, and key symbology from here — one-off
//! inline values are a review rejection.

use bevy::feathers::palette;
use bevy::prelude::*;
use editor_api::keymap::{Binding, Chord};

/// Spacing scale (px). Compose layouts only from these.
pub mod space {
    pub const XS: f32 = 4.0;
    pub const S: f32 = 8.0;
    pub const M: f32 = 12.0;
}

/// Corner radius scale (px): S for chips/rows, L for panels.
pub mod radius {
    pub const S: f32 = 4.0;
    pub const L: f32 = 6.0;
}

pub const BAR_HEIGHT: f32 = 30.0;

/// Nerd-font glyphs (FontAwesome range — present in all nerd fonts). Symbols over
/// words (design bar); named here so widgets never embed raw codepoints.
pub mod glyph {
    /// Palette: command/action search.
    pub const SEARCH: &str = "\u{f002}";
    /// Palette: insert object/asset.
    pub const INSERT: &str = "\u{f067}";
    /// Palette: component browse/insert.
    pub const COMPONENT: &str = "\u{f12e}";
}

/// Type scale (logical px). Chrome text never uses ad-hoc sizes.
pub mod font_size {
    /// Chips, badges, secondary metadata.
    pub const XS: f32 = 11.0;
    /// Status bar, hints, binding glyphs, list metadata.
    pub const S: f32 = 13.0;
    /// Body/list text.
    pub const M: f32 = 14.0;
}

/// Semantic chrome colors derived from the feathers palette — never ad-hoc RGB.
pub mod color {
    use super::*;
    pub const TEXT_DIM: Color = Color::srgb(0.55, 0.57, 0.62);
    pub const TEXT_KEYS: Color = Color::srgb(0.72, 0.74, 0.80);
    pub const TEXT_ON_ACCENT: Color = Color::srgb(0.05, 0.05, 0.08);
    pub const TEXT_WARN: Color = Color::srgb(0.83, 0.60, 0.42);
    pub fn accent() -> Color {
        palette::ACCENT
    }
    pub fn selection() -> Color {
        palette::ACCENT.with_alpha(0.30)
    }
}

/// Elevation for floating surfaces (palette, which-key, future popups): drop shadow
/// + hairline edge. One treatment, applied by every floating panel.
pub fn floating_shadow() -> bevy::ui::BoxShadow {
    bevy::ui::BoxShadow::new(
        Color::BLACK.with_alpha(0.55),
        bevy::ui::px(0),
        bevy::ui::px(10),
        bevy::ui::px(4),
        bevy::ui::px(28),
    )
}

/// Hairline edge color for floating surfaces (subtle light line, not a widget border).
pub const HAIRLINE: Color = Color::srgba(1.0, 1.0, 1.0, 0.09);

/// The editor's chrome fonts, loaded once at startup: Inter for UI text (modern,
/// OFL) and FiraCode Nerd Font for keys/glyph symbology. Never use Bevy's built-in
/// default font (a Fira Mono subset) for chrome.
#[derive(Resource, Clone)]
pub struct UiFonts {
    pub sans: Handle<Font>,
    pub sans_medium: Handle<Font>,
    pub mono: Handle<Font>,
}

pub const SANS_PATH: &str = "fonts/Inter-Regular.ttf";
pub const SANS_MEDIUM_PATH: &str = "fonts/Inter-Medium.ttf";
pub const MONO_PATH: &str = "fonts/FiraCodeNerdFont-Regular.ttf";

pub fn load_ui_fonts(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(UiFonts {
        sans: assets.load(SANS_PATH),
        sans_medium: assets.load(SANS_MEDIUM_PATH),
        mono: assets.load(MONO_PATH),
    });
}

fn text_font(handle: &Handle<Font>, size: f32) -> TextFont {
    TextFont {
        font: bevy::text::FontSource::Handle(handle.clone()),
        font_size: bevy::text::FontSize::Px(size),
        ..Default::default()
    }
}

/// Glyph/key text (mono, nerd symbols).
pub fn mono(fonts: &UiFonts, size: f32) -> TextFont {
    text_font(&fonts.mono, size)
}

/// Body chrome text.
pub fn sans(fonts: &UiFonts, size: f32) -> TextFont {
    text_font(&fonts.sans, size)
}

/// Emphasized chrome text (titles, chips, selected labels).
pub fn sans_medium(fonts: &UiFonts, size: f32) -> TextFont {
    text_font(&fonts.sans_medium, size)
}

/// Key symbology (design bar: symbols over words). Falls back to the config-file
/// spelling for anything without an established glyph.
pub fn pretty_chord(chord: &Chord) -> String {
    let mut out = String::new();
    if chord.modifiers.ctrl {
        out.push('⌃');
    }
    if chord.modifiers.alt {
        out.push('⌥');
    }
    if chord.modifiers.shift {
        out.push('⇧');
    }
    if chord.modifiers.cmd {
        out.push('⌘');
    }
    use bevy::input::keyboard::KeyCode as K;
    let key = match chord.key {
        K::Space => "␣".into(),
        K::Enter => "⏎".into(),
        K::Escape => "⎋".into(),
        K::Backspace => "⌫".into(),
        K::Delete => "⌦".into(),
        K::Tab => "⇥".into(),
        K::ArrowUp => "↑".into(),
        K::ArrowDown => "↓".into(),
        K::ArrowLeft => "←".into(),
        K::ArrowRight => "→".into(),
        _ => {
            // Reuse the canonical config-file spelling for letters/digits/f-keys.
            let plain = Chord { modifiers: Default::default(), key: chord.key };
            let s = plain.to_string();
            s
        }
    };
    out.push_str(&key);
    out
}

pub fn pretty_binding(binding: &Binding) -> String {
    binding.0.iter().map(pretty_chord).collect::<Vec<_>>().join(" ")
}

pub fn pretty_chords(chords: &[Chord]) -> String {
    chords.iter().map(pretty_chord).collect::<Vec<_>>().join(" ")
}
