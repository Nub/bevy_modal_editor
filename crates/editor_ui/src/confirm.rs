//! The delete question (owner direction).
//!
//! `d` is one keystroke from losing work, and the selection it acts on is not
//! always the one you think — a box-drag catches more than it looks like, and
//! `*` can take fifty objects at once. So `d` asks, and the question NAMES what
//! would go rather than counting it.
//!
//! A second `d` answers yes. That is not a `d d` key sequence: the keymap
//! refuses to bind a chord that is a strict prefix of another, and rightly —
//! so the fast path comes from the dialog answering rather than the resolver
//! matching. The result is what the hand wants anyway: `dd` deletes at once,
//! and a lone `d` stops and shows you the stakes.

use crate::style::{self, UiFonts};
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui::widget::Text;
use bevy::ui::{BorderRadius, PositionType, UiRect, percent, px};
use editor_core::clipboard::DeleteConfirm;
use editor_core::prelude::*;

#[derive(Component)]
pub(crate) struct ConfirmRoot;

#[derive(Component)]
pub(crate) struct ConfirmText;

pub(crate) fn spawn_confirm(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    settings: Res<EditorSettings>,
) {
    let ui = settings.ui.clone();
    commands
        .spawn((
            ConfirmRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: percent(38.0),
                justify_content: JustifyContent::Center,
                ..default()
            },
            // Above the palette: this is the one thing that should not be
            // behind anything.
            GlobalZIndex(320),
            Visibility::Hidden,
            bevy::ui::UiTransform::IDENTITY,
            crate::appear::FloatingSurface::default(),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(style::space::S),
                        padding: UiRect::all(px(style::space::M)),
                        border: UiRect::all(px(1.0)),
                        border_radius: BorderRadius::all(px(style::radius::L)),
                        min_width: px(360.0),
                        ..default()
                    },
                    ThemeBackgroundColor(tokens::WINDOW_BG),
                    // The warn tone, because this is the one dialog that ends
                    // with something gone.
                    BorderColor::all(style::color::TEXT_WARN.with_alpha(0.55)),
                    style::floating_shadow(),
                ))
                .with_children(|panel| {
                    panel.spawn((
                        ConfirmText,
                        Text::new(String::new()),
                        style::sans_medium(&fonts, ui.font_size_m),
                        TextColor(style::color::TEXT_BRIGHT),
                    ));
                    panel.spawn((
                        Text::new("d again to delete \u{b7} \u{238b} to keep it"),
                        style::mono(&fonts, ui.font_size_s),
                        TextColor(style::color::TEXT_DIM),
                    ));
                });
        });
}

/// Show the question while one is pending, and say what it would take.
pub(crate) fn sync_confirm(
    confirm: Res<DeleteConfirm>,
    mut root: Query<&mut Visibility, With<ConfirmRoot>>,
    mut text: Query<&mut Text, With<ConfirmText>>,
) {
    if !confirm.is_changed() {
        return;
    }
    let showing = confirm.pending.is_some();
    for mut visibility in &mut root {
        let target = if showing {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
        if *visibility != target {
            *visibility = target;
        }
    }
    if let Some(prompt) = &confirm.pending {
        for mut label in &mut text {
            label.0 = prompt.summary();
        }
    }
}
