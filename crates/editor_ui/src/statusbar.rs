//! Status bar (spec §7 design bar): themed chrome strip that ALWAYS states the
//! current activity — mode chip, live hint, pending keys / transient feedback /
//! selection count, and the unsaved-changes dot.

use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui::px;
use editor_core::prelude::*;

use crate::style::{self, UiFonts};

#[derive(Component, Default, Clone)]
pub(crate) struct StatusBar;
#[derive(Component, Default, Clone)]
pub(crate) struct StatusModeText;
#[derive(Component, Default, Clone)]
pub(crate) struct StatusHint;
#[derive(Component, Default, Clone)]
pub(crate) struct StatusKeys;
#[derive(Component, Default, Clone)]
pub(crate) struct StatusDirty;

/// Transient status feedback (save/load results). Shown in the keys slot when no
/// sequence is pending.
#[derive(Resource, Default)]
pub(crate) struct StatusFlash {
    text: String,
    success: bool,
    until: f32,
}

pub(crate) fn spawn_statusbar(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            StatusBar,
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(0),
                height: px(style::BAR_HEIGHT),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(style::space::M),
                padding: UiRect::horizontal(px(style::space::M)),
                ..default()
            },
            ThemeBackgroundColor(tokens::PANE_HEADER_BG),
            GlobalZIndex(100),
            Visibility::Hidden,
        ))
        .with_children(|bar| {
            bar.spawn((
                Node {
                    padding: UiRect::axes(px(style::space::S), px(2.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(px(style::radius::S)),
                    ..default()
                },
                BackgroundColor(style::color::accent()),
            ))
            .with_children(|chip| {
                chip.spawn((
                    StatusModeText,
                    Text::new("NORMAL"),
                    style::sans_medium(&fonts, style::font_size::XS),
                    TextColor(style::color::TEXT_ON_ACCENT),
                ));
            });
            bar.spawn((
                StatusDirty,
                Text::new("●"),
                style::sans(&fonts, style::font_size::S),
                TextColor(style::color::TEXT_WARN),
                Visibility::Hidden,
            ));
            bar.spawn((
                StatusHint,
                Text::new(""),
                ThemedText,
                style::sans(&fonts, style::font_size::S),
                TextColor(style::color::TEXT_DIM),
            ));
            bar.spawn(Node { flex_grow: 1.0, ..default() });
            bar.spawn((
                StatusKeys,
                Text::new(""),
                style::mono(&fonts, style::font_size::S),
                TextColor(style::color::TEXT_KEYS),
            ));
        });
}

pub(crate) fn collect_io_feedback(
    mut reader: MessageReader<editor_scene::SceneIoFeedback>,
    time: Res<Time>,
    mut flash: ResMut<StatusFlash>,
) {
    for feedback in reader.read() {
        flash.text = feedback.message.clone();
        flash.success = feedback.success;
        flash.until = time.elapsed_secs() + 3.0;
    }
}

#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct StatusData<'w> {
    state: Res<'w, EditorState>,
    mode: Res<'w, CurrentMode>,
    modes: Res<'w, Modes>,
    pending: Res<'w, PendingKeys>,
    gesture: Res<'w, MoveGesture>,
    insert: Res<'w, InsertState>,
    kinds: Res<'w, KindCatalog>,
    flash: Res<'w, StatusFlash>,
    dirty: Res<'w, editor_scene::SceneDirty>,
    time: Res<'w, Time>,
}

#[allow(clippy::type_complexity)]
pub(crate) fn update_statusbar(
    data: StatusData,
    selected: Query<(), With<Selected>>,
    mut bar: Query<&mut Visibility, With<StatusBar>>,
    mut mode_text: Query<
        &mut Text,
        (With<StatusModeText>, Without<StatusHint>, Without<StatusKeys>),
    >,
    mut hint_text: Query<
        &mut Text,
        (With<StatusHint>, Without<StatusModeText>, Without<StatusKeys>),
    >,
    mut keys_text: Query<
        (&mut Text, &mut TextColor),
        (With<StatusKeys>, Without<StatusModeText>, Without<StatusHint>),
    >,
    mut dirty_dot: Query<
        &mut Visibility,
        (With<StatusDirty>, Without<StatusBar>),
    >,
) {
    for mut visibility in &mut bar {
        *visibility = if data.state.active { Visibility::Visible } else { Visibility::Hidden };
    }
    if !data.state.active {
        return;
    }

    // The bar always states the current activity (owner rule): an active gesture or
    // an armed insert kind owns the chip + hint; the plain mode otherwise.
    let gesture_active = !matches!(*data.gesture, MoveGesture::Idle);
    let inserting = (data.mode.0 == MODE_INSERT)
        .then(|| data.insert.kind.as_ref())
        .flatten()
        .and_then(|id| data.kinds.get(id))
        .map(|k| k.display_name);
    let mode_def = data.modes.get(&data.mode.0);
    for mut text in &mut mode_text {
        let name = if gesture_active {
            "MOVE".to_string()
        } else if inserting.is_some() {
            "INSERT".to_string()
        } else {
            mode_def.map(|m| m.name.to_uppercase()).unwrap_or_else(|| "?".into())
        };
        if text.0 != name {
            text.0 = name;
        }
    }
    for mut text in &mut hint_text {
        let hint = if gesture_active {
            "moving selection · x/y/z constrain · click ⏎ commit · ⎋ cancel".to_string()
        } else if let Some(kind_name) = inserting {
            format!("inserting {kind_name} · click place · ⇧click multi · ⎋ done")
        } else {
            mode_def.map(|m| m.statusline_hint.to_string()).unwrap_or_default()
        };
        if text.0 != hint {
            text.0 = hint;
        }
    }
    for mut visibility in &mut dirty_dot {
        *visibility = if data.dirty.0 { Visibility::Visible } else { Visibility::Hidden };
    }
    // Keys slot: pending glyphs win; then selection count; then transient feedback.
    let selection_count = selected.iter().count();
    for (mut text, mut color) in &mut keys_text {
        if !data.pending.0.is_empty() {
            let pending_text = style::pretty_chords(&data.pending.0);
            if text.0 != pending_text {
                text.0 = pending_text;
                color.0 = style::color::TEXT_KEYS;
            }
        } else if data.time.elapsed_secs() < data.flash.until {
            if text.0 != data.flash.text {
                text.0 = data.flash.text.clone();
                color.0 = if data.flash.success {
                    Color::srgb(0.55, 0.78, 0.55)
                } else {
                    style::color::TEXT_WARN
                };
            }
        } else if selection_count > 0 {
            let label = format!("{selection_count} selected");
            if text.0 != label {
                text.0 = label;
                color.0 = style::color::TEXT_KEYS;
            }
        } else if !text.0.is_empty() {
            text.0.clear();
        }
    }
}
