//! Which-key: an nvim-style popup above the status bar. Shown while a prefix is
//! pending (its continuations) AND after an unbound press (everything available in
//! the current context — teach, don't scold).

use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui::px;
use editor_core::prelude::*;

use crate::style::{self, UiFonts};

#[derive(Component, Default, Clone)]
pub(crate) struct WhichKeyPanel;

#[derive(Default, PartialEq, Clone)]
struct WhichKeyContent {
    header: String,
    header_warn: bool,
    entries: Vec<(String, String)>, // (key glyphs, action name)
}

#[derive(Resource, Default)]
pub(crate) struct WhichKey {
    content: Option<WhichKeyContent>,
    /// For the unbound case: auto-dismiss deadline (seconds of app time).
    until: Option<f32>,
    dirty: bool,
}

pub(crate) fn spawn_which_key(mut commands: Commands) {
    commands.spawn((
        WhichKeyPanel,
        Node {
            position_type: PositionType::Absolute,
            left: px(style::space::M),
            right: px(style::space::M),
            bottom: px(style::BAR_HEIGHT + style::space::S),
            flex_direction: FlexDirection::Column,
            row_gap: px(style::space::S),
            padding: UiRect::all(px(style::space::M)),
            border: UiRect::all(px(1.0)),
            border_radius: BorderRadius::all(px(style::radius::L)),
            ..default()
        },
        ThemeBackgroundColor(tokens::WINDOW_BG),
        BorderColor::all(style::HAIRLINE),
        style::floating_shadow(),
        GlobalZIndex(150),
        Visibility::Hidden,
    ));
}

fn entries_for(
    keymap: &ResolvedKeymap,
    catalog: &ActionCatalog,
    contexts: &[ContextId],
    pending: &[editor_api::keymap::Chord],
) -> Vec<(String, String)> {
    which_key_continuations(keymap, contexts, pending)
        .iter()
        .map(|(chord, binding, action)| {
            let name = catalog.get(action).map(|d| d.name.as_ref()).unwrap_or(action.as_str());
            // Show the immediate next key; if the binding goes deeper, mark it as a
            // group with an ellipsis (nvim-which-key idiom).
            let deeper = binding.0.len() > pending.len() + 1;
            let key = if deeper {
                format!("{} …", style::pretty_chord(chord))
            } else {
                style::pretty_chord(chord)
            };
            (key, name.to_string())
        })
        .collect()
}

pub(crate) fn compute_which_key(
    state: Res<EditorState>,
    mode: Res<CurrentMode>,
    overlay: Res<OverlayContext>,
    pending: Res<PendingKeys>,
    keymap: Res<ResolvedKeymap>,
    catalog: Res<ActionCatalog>,
    panel_focus: Res<PanelFocus>,
    panel_catalog: Res<PanelCatalog>,
    mut unresolved: MessageReader<KeysUnresolved>,
    mut actions: MessageReader<ActionInvoked>,
    time: Res<Time>,
    settings: Res<EditorSettings>,
    mut which_key: ResMut<WhichKey>,
) {
    // Escape dismisses the popup instantly (universal backout).
    if actions.read().any(|a| a.action.as_str() == "core.escape-home") {
        which_key.until = None;
    }
    let now = time.elapsed_secs();
    let contexts = active_contexts(&state, &mode, &overlay, &panel_focus, &panel_catalog);

    let next = if !state.active {
        None
    } else if !pending.0.is_empty() {
        // A live prefix: show its continuations.
        Some((
            WhichKeyContent {
                header: style::pretty_chords(&pending.0),
                header_warn: false,
                entries: entries_for(&keymap, &catalog, &contexts, &pending.0),
            },
            None,
        ))
    } else if let Some(last) = unresolved.read().last() {
        // Unbound press: teach what IS available in this context.
        Some((
            WhichKeyContent {
                header: format!(
                    "{}  ·  unbound — available here:",
                    style::pretty_chords(&last.0)
                ),
                header_warn: true,
                entries: entries_for(&keymap, &catalog, &contexts, &[]),
            },
            Some(now + settings.ui.which_key_dismiss_secs),
        ))
    } else if which_key.until.is_some_and(|until| now < until) {
        return; // keep showing the unbound popup until its deadline
    } else {
        None
    };

    match next {
        Some((content, until)) => {
            if which_key.content.as_ref() != Some(&content) {
                which_key.content = Some(content);
                which_key.dirty = true;
            }
            which_key.until = until;
        }
        None => {
            if which_key.content.is_some() {
                which_key.content = None;
                which_key.until = None;
                which_key.dirty = true;
            }
        }
    }
}

pub(crate) fn rebuild_which_key(
    mut which_key: ResMut<WhichKey>,
    panel: Single<(Entity, &mut Visibility), With<WhichKeyPanel>>,
    fonts: Res<UiFonts>,
    settings: Res<EditorSettings>,
    mut commands: Commands,
) {
    if !which_key.dirty {
        return;
    }
    let ui = settings.ui.clone();
    which_key.dirty = false;
    let (panel_entity, mut visibility) = panel.into_inner();
    commands.entity(panel_entity).despawn_related::<Children>();

    let Some(content) = &which_key.content else {
        *visibility = Visibility::Hidden;
        return;
    };
    *visibility = Visibility::Visible;

    let header = content.header.clone();
    let header_color =
        if content.header_warn { style::color::TEXT_WARN } else { style::color::TEXT_DIM };
    let entries = content.entries.clone();
    let key_font = style::mono(&fonts, ui.font_size_s);

    commands.entity(panel_entity).with_children(|panel| {
        panel.spawn((
            Text::new(header),
            style::mono(&fonts, ui.font_size_xs),
            TextColor(header_color),
        ));
        panel
            .spawn(Node {
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(style::space::M),
                row_gap: px(style::space::XS),
                ..default()
            })
            .with_children(|grid| {
                for (key, name) in entries {
                    grid.spawn(Node {
                        width: px(220.0),
                        align_items: AlignItems::Center,
                        column_gap: px(style::space::S),
                        ..default()
                    })
                    .with_children(|cell| {
                        cell.spawn((
                            Node {
                                min_width: px(28.0),
                                justify_content: JustifyContent::Center,
                                padding: UiRect::axes(px(style::space::XS), px(1.0)),
                                border_radius: BorderRadius::all(px(style::radius::S)),
                                ..default()
                            },
                            BackgroundColor(style::color::selection()),
                        ))
                        .with_children(|badge| {
                            badge.spawn((
                                Text::new(key),
                                key_font.clone(),
                                TextColor(style::color::TEXT_KEYS),
                            ));
                        });
                        cell.spawn((
                            Text::new(name),
                            style::sans(&fonts, ui.font_size_s),
                            TextColor(style::color::TEXT_DIM),
                        ));
                    });
                }
                if content.entries.is_empty() {
                    grid.spawn((
                        Text::new("nothing bound in this context"),
                        style::sans(&fonts, ui.font_size_s),
                        TextColor(style::color::TEXT_DIM),
                    ));
                }
            });
    });
}
