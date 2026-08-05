//! The single source of editor-chrome styling (design bar, spec §7): every widget
//! draws its spacing, radii, colors, fonts, and key symbology from here — one-off
//! inline values are a review rejection.

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

/// THE chrome palette — one disciplined set, never ad-hoc RGB. Neutral dark
/// ramp (no color cast), one calibrated accent, three text tiers.
pub mod color {
    use super::*;
    /// Secondary text/labels — readable, never competing with content.
    pub const TEXT_DIM: Color = Color::srgb(0.58, 0.59, 0.63);
    /// Primary chrome text (names, values, keys).
    pub const TEXT_KEYS: Color = Color::srgb(0.82, 0.83, 0.86);
    /// Emphasized text (selected rows, titles) — bright neutral, NOT accent:
    /// accent-colored text is reserved for identity marks (◆, prefab names).
    pub const TEXT_BRIGHT: Color = Color::srgb(0.91, 0.92, 0.94);
    pub const TEXT_ON_ACCENT: Color = Color::srgb(0.04, 0.06, 0.10);
    pub const TEXT_WARN: Color = Color::srgb(0.85, 0.62, 0.42);
    pub const TEXT_OK: Color = Color::srgb(0.55, 0.78, 0.55);
    /// Quiet chip fill for at-rest states (the NORMAL mode chip): the resting
    /// state must be the quietest thing on screen — accent fills mean departure.
    pub const CHIP_REST: Color = Color::srgba(1.0, 1.0, 1.0, 0.07);
    /// Calibrated soft blue — luminous on the dark ramp without the stock
    /// bootstrap harshness.
    pub fn accent() -> Color {
        Color::srgb(0.42, 0.62, 0.95)
    }
    pub fn selection() -> Color {
        accent().with_alpha(0.22)
    }
}

/// Elevation for floating surfaces (palette, which-key, future popups): drop shadow
/// + hairline edge. One treatment, applied by every floating panel.
pub fn floating_shadow() -> bevy::ui::BoxShadow {
    // Layered: a tight contact shadow grounds the surface, a wide ambient one
    // lifts it — single hard shadows read flat.
    bevy::ui::BoxShadow(vec![
        bevy::ui::ShadowStyle {
            color: Color::BLACK.with_alpha(0.35),
            x_offset: bevy::ui::px(0),
            y_offset: bevy::ui::px(2),
            spread_radius: bevy::ui::px(0),
            blur_radius: bevy::ui::px(6),
        },
        bevy::ui::ShadowStyle {
            color: Color::BLACK.with_alpha(0.45),
            x_offset: bevy::ui::px(0),
            y_offset: bevy::ui::px(14),
            spread_radius: bevy::ui::px(2),
            blur_radius: bevy::ui::px(40),
        },
    ])
}

/// A TOOL, not a default (owner): reach for it only where a surface needs
/// modeled light to read — never as blanket decoration.
pub fn header_gradient() -> bevy::ui::BackgroundGradient {
    use bevy::ui::{BackgroundGradient, ColorStop, Gradient, LinearGradient};
    BackgroundGradient(vec![Gradient::Linear(LinearGradient {
        color_space: default(),
        angle: LinearGradient::TO_TOP,
        stops: vec![
            ColorStop::new(Color::srgba(1.0, 1.0, 1.0, 0.0), bevy::ui::percent(0)),
            ColorStop::new(Color::srgba(1.0, 1.0, 1.0, 0.03), bevy::ui::percent(100)),
        ],
    })])
}

/// A TOOL, not a default (owner): only where a control's affordance needs it.
pub fn accent_gradient() -> bevy::ui::BackgroundGradient {
    use bevy::ui::{BackgroundGradient, ColorStop, Gradient, LinearGradient};
    BackgroundGradient(vec![Gradient::Linear(LinearGradient {
        color_space: default(),
        angle: LinearGradient::TO_TOP,
        stops: vec![
            ColorStop::new(Color::srgba(1.0, 1.0, 1.0, 0.0), bevy::ui::percent(0)),
            ColorStop::new(Color::srgba(1.0, 1.0, 1.0, 0.10), bevy::ui::percent(100)),
        ],
    })])
}

/// Chrome labels never wrap (owner: no text wrapping in confusing places) —
/// a label that can't fit must be laid out differently, not broken mid-phrase.
pub fn no_wrap() -> bevy::text::TextLayout {
    bevy::text::TextLayout {
        linebreak: bevy::text::LineBreak::NoWrap,
        ..Default::default()
    }
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

// Embedded (registered in `EditorUiPlugin`): the chrome fonts ship inside the crate —
// games never copy font files into their asset folders.
pub const SANS_PATH: &str = "embedded://editor_ui/fonts/Inter-Regular.ttf";
pub const SANS_MEDIUM_PATH: &str = "embedded://editor_ui/fonts/Inter-Medium.ttf";
pub const MONO_PATH: &str = "embedded://editor_ui/fonts/FiraCodeNerdFont-Regular.ttf";

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
/// Fold chevrons — nerd-font codepoints (guaranteed in FiraCode NF; the
/// BMP triangles U+25B8/25BE are NOT in it and render tofu).
pub const CHEVRON_DOWN: &str = "\u{f078}";
pub const CHEVRON_RIGHT: &str = "\u{f054}";

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
            let plain = Chord {
                modifiers: Default::default(),
                key: chord.key,
            };

            plain.to_string()
        }
    };
    out.push_str(&key);
    out
}

pub fn pretty_binding(binding: &Binding) -> String {
    binding
        .0
        .iter()
        .map(pretty_chord)
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn pretty_chords(chords: &[Chord]) -> String {
    chords
        .iter()
        .map(pretty_chord)
        .collect::<Vec<_>>()
        .join(" ")
}
