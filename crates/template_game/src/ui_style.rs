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

pub const BAR_HEIGHT: f32 = 28.0;

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

/// The editor's icon-capable UI font (FiraCode Nerd Font, OFL). Loaded once at
/// startup; every chrome text that shows keys/symbols uses it.
#[derive(Resource, Clone)]
pub struct UiFont(pub Handle<Font>);

pub fn load_ui_font(mut commands: Commands, assets: Res<AssetServer>) {
    commands.insert_resource(UiFont(assets.load("fonts/FiraCodeNerdFont-Regular.ttf")));
}

/// The one way chrome text adopts the UI font (0.19: `TextFont` takes a `FontSource`).
pub fn text_font(font: &UiFont) -> TextFont {
    TextFont { font: bevy::text::FontSource::Handle(font.0.clone()), ..Default::default() }
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
