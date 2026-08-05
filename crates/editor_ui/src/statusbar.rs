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
pub(crate) struct StatusModeChip;
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
    pub(crate) text: String,
    success: bool,
    until: f32,
}

pub(crate) fn spawn_statusbar(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    settings: Res<EditorSettings>,
) {
    let ui = settings.ui.clone();
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
                StatusModeChip,
                Node {
                    padding: UiRect::axes(px(style::space::S), px(2.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border_radius: BorderRadius::all(px(style::radius::S)),
                    ..default()
                },
                // At rest the chip is quiet; update_statusbar swaps in the accent
                // fill for any departure (gesture, insert, editing, panel focus).
                BackgroundColor(style::color::CHIP_REST),
            ))
            .with_children(|chip| {
                chip.spawn((
                    StatusModeText,
                    Text::new("NORMAL"),
                    style::sans_medium(&fonts, ui.font_size_xs),
                    TextColor(style::color::TEXT_KEYS),
                ));
            });
            bar.spawn((
                StatusDirty,
                Text::new("●"),
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_WARN),
                Visibility::Hidden,
            ));
            bar.spawn((
                StatusHint,
                Text::new(""),
                ThemedText,
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
            ));
            bar.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            bar.spawn((
                StatusKeys,
                Text::new(""),
                style::mono(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_KEYS),
            ));
        });
}

pub(crate) fn collect_io_feedback(
    mut reader: MessageReader<editor_scene::SceneIoFeedback>,
    mut unresolved: MessageReader<KeysUnresolved>,
    time: Res<Time>,
    settings: Res<EditorSettings>,
    mut flash: ResMut<StatusFlash>,
) {
    for feedback in reader.read() {
        flash.text = feedback.message.clone();
        flash.success = feedback.success;
        flash.until = time.elapsed_secs() + settings.ui.status_flash_secs;
    }
    // Unbound keys flash quietly here — feedback without a popup (owner). Bare
    // digits are RESERVED for count prefixes (keymap doc): flashing them as
    // unbound reads as breakage for a planned feature, so they stay silent.
    let is_bare_digit = |chord: &editor_api::keymap::Chord| {
        chord.modifiers == editor_api::keymap::Modifiers::default()
            && matches!(
                chord.key,
                KeyCode::Digit0
                    | KeyCode::Digit1
                    | KeyCode::Digit2
                    | KeyCode::Digit3
                    | KeyCode::Digit4
                    | KeyCode::Digit5
                    | KeyCode::Digit6
                    | KeyCode::Digit7
                    | KeyCode::Digit8
                    | KeyCode::Digit9
            )
    };
    if let Some(keys) = unresolved
        .read()
        .filter(|k| !k.0.iter().all(is_bare_digit))
        .last()
    {
        flash.text = format!("{} · unbound", style::pretty_chords(&keys.0));
        flash.success = false;
        flash.until = time.elapsed_secs() + settings.ui.status_flash_secs * 0.5;
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
    panel_focus: Res<'w, PanelFocus>,
    panel_catalog: Res<'w, PanelCatalog>,
    open_instance: Res<'w, editor_prefabs::open_mode::OpenInstance>,
    grid_snap: Res<'w, GridSnap>,
    settings: Res<'w, EditorSettings>,
}

#[allow(clippy::type_complexity)]
pub(crate) fn update_statusbar(
    data: StatusData,
    selected: Query<(), With<Selected>>,
    mut bar: Query<&mut Visibility, With<StatusBar>>,
    mut mode_chip: Query<&mut BackgroundColor, With<StatusModeChip>>,
    mut mode_text: Query<
        (&mut Text, &mut TextColor),
        (
            With<StatusModeText>,
            Without<StatusHint>,
            Without<StatusKeys>,
        ),
    >,
    mut hint_text: Query<
        &mut Text,
        (
            With<StatusHint>,
            Without<StatusModeText>,
            Without<StatusKeys>,
        ),
    >,
    mut keys_text: Query<
        (&mut Text, &mut TextColor),
        (
            With<StatusKeys>,
            Without<StatusModeText>,
            Without<StatusHint>,
        ),
    >,
    mut dirty_dot: Query<&mut Visibility, (With<StatusDirty>, Without<StatusBar>)>,
) {
    for mut visibility in &mut bar {
        *visibility = if data.state.active {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !data.state.active {
        return;
    }

    // The bar always states the current activity (owner rule): an active gesture or
    // an armed insert kind owns the chip + hint; the plain mode otherwise.
    let gesture_active = !matches!(*data.gesture, MoveGesture::Idle);
    let open_prefab = data.open_instance.0.as_ref().map(|o| o.name.clone());
    let focused_panel = data
        .panel_focus
        .0
        .as_ref()
        .and_then(|id| data.panel_catalog.get(id))
        .map(|decl| decl.title);
    let inserting = (data.mode.0 == MODE_INSERT)
        .then(|| data.insert.kind.as_ref())
        .flatten()
        .and_then(|id| data.kinds.get(id))
        .map(|k| k.display_name);
    let mode_def = data.modes.get(&data.mode.0);
    // The resting state is the quietest thing on screen; the accent fill is the
    // signal that the editor has left NORMAL (design bar).
    let at_rest = !gesture_active
        && open_prefab.is_none()
        && focused_panel.is_none()
        && inserting.is_none()
        && data.mode.0 == MODE_NORMAL;
    for mut chip_bg in &mut mode_chip {
        let want = if at_rest {
            style::color::CHIP_REST
        } else {
            style::color::accent()
        };
        if chip_bg.0 != want {
            chip_bg.0 = want;
        }
    }
    for (mut text, mut text_color) in &mut mode_text {
        let want = if at_rest {
            style::color::TEXT_KEYS
        } else {
            style::color::TEXT_ON_ACCENT
        };
        if text_color.0 != want {
            text_color.0 = want;
        }
        let name = if gesture_active {
            match &*data.gesture {
                MoveGesture::Active { axis, typed, .. } => {
                    let mut label = "MOVE".to_string();
                    if let Some(axis) = axis {
                        label.push_str([" · X", " · Y", " · Z"][*axis]);
                    }
                    if !typed.is_empty() {
                        label.push_str(&format!(" {typed}"));
                    }
                    label
                }
                MoveGesture::Idle => "MOVE".to_string(),
            }
        } else if let Some(prefab) = &open_prefab {
            format!("EDITING ◆ {}", prefab.to_uppercase())
        } else if let Some(title) = focused_panel {
            title.to_uppercase()
        } else if inserting.is_some() {
            "INSERT".to_string()
        } else {
            mode_def
                .map(|m| m.name.to_uppercase())
                .unwrap_or_else(|| "?".into())
        };
        if text.0 != name {
            text.0 = name;
        }
    }
    // STATUS, never key hints (owner 2026-08-04): which-key owns discoverability;
    // this slot reports live editor state that has no other visual — snap state,
    // the armed insert kind.
    for mut text in &mut hint_text {
        let mut parts: Vec<String> = Vec::new();
        if let Some(kind_name) = inserting {
            parts.push(format!("inserting {kind_name}"));
        }
        if data.grid_snap.enabled {
            parts.push(format!("snap {}m", data.settings.viewport.grid_step));
        }
        let status = parts.join("  ·  ");
        if text.0 != status {
            text.0 = status;
        }
    }
    for mut visibility in &mut dirty_dot {
        *visibility = if data.dirty.0 {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
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
                // Success speaks in the content tier — the message IS the
                // signal; a hue per outcome read as terminal noise (owner).
                color.0 = if data.flash.success {
                    style::color::TEXT_KEYS
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
