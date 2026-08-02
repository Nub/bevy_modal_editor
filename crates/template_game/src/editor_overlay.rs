//! The editor inside the game (spec §1) — only compiled with `--features editor`.
//!
//! M1 scope: the kernel (modes, resolver, keymaps), the command palette, and a real
//! status bar. Chrome follows the standing design bar (spec §7): themed surfaces,
//! never floating text over the render.

use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::{dark_theme::create_dark_theme, theme::UiTheme, tokens, FeathersPlugins};
use bevy::prelude::*;
use bevy::ui::px;
use editor_core::prelude::*;

use crate::game::GameInputActive;
use crate::ui_style::{self as style};

#[derive(Component, Default, Clone)]
struct StatusBar;
#[derive(Component, Default, Clone)]
struct StatusModeChip;
#[derive(Component, Default, Clone)]
struct StatusModeText;
#[derive(Component, Default, Clone)]
struct StatusHint;
#[derive(Component, Default, Clone)]
struct StatusKeys;

pub struct EditorOverlayPlugin;

impl Plugin for EditorOverlayPlugin {
    fn build(&self, app: &mut App) {
        // User keymap layer (M1 acceptance: rebind without recompiling): overrides
        // in ./editor-keymap.ron win over registry defaults; delete to restore.
        app.insert_resource(KeymapPaths { user: Some("editor-keymap.ron".into()) });
        app.add_plugins(FeathersPlugins)
            .insert_resource(UiTheme(create_dark_theme()));
        app.add_plugins((EditorCorePlugin, crate::palette::PalettePlugin))
            .add_systems(
                Update,
                (sync_game_input, collect_unresolved, update_statusbar)
                    .chain()
                    .in_set(editor_core::EditorSet::Sync),
            );
        app.init_resource::<UnboundFlash>();
        app.add_systems(Startup, (style::load_ui_font, spawn_statusbar).chain());
    }
}

fn spawn_statusbar(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        StatusBar
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
        }
        ThemeBackgroundColor(tokens::PANE_HEADER_BG)
        GlobalZIndex(100)
        Visibility::Hidden
        Children [
            // mode chip: accent-filled rounded tag
            (
                StatusModeChip
                Node {
                    padding: UiRect::axes(px(style::space::S), px(2.0)),
                    align_items: AlignItems::Center,
                    border_radius: {BorderRadius::all(px(style::radius::S))},
                }
                BackgroundColor({style::color::accent()})
                Children [
                    (StatusModeText Text("NORMAL") TextColor({style::color::TEXT_ON_ACCENT}))
                ]
            ),
            (StatusHint Text("") ThemedText TextColor({style::color::TEXT_DIM})),
            (Node { flex_grow: 1.0 }),
            (
                StatusKeys Text("")
                template(|ctx| {
                    Ok(bevy::text::TextFont {
                        font: bevy::text::FontSource::Handle(
                            ctx.resource::<AssetServer>()
                                .load("fonts/FiraCodeNerdFont-Regular.ttf"),
                        ),
                        ..Default::default()
                    })
                })
                TextColor({style::color::TEXT_KEYS})
            ),
        ]
    });
}

/// Brief "unbound" readout — every keypress deserves feedback (design bar).
#[derive(Resource, Default)]
struct UnboundFlash {
    text: String,
    until: f32,
}

fn collect_unresolved(
    mut reader: MessageReader<KeysUnresolved>,
    time: Res<Time>,
    mut flash: ResMut<UnboundFlash>,
) {
    for unresolved in reader.read() {
        flash.text = format!("{}  ·  unbound", style::pretty_chords(&unresolved.0));
        flash.until = time.elapsed_secs() + 1.6;
    }
}

/// The editor owns input while active; the game module knows nothing about the
/// editor — it just honors `GameInputActive`.
fn sync_game_input(state: Res<EditorState>, mut game_input: ResMut<GameInputActive>) {
    let game_owns = !state.active;
    if game_input.0 != game_owns {
        game_input.0 = game_owns;
    }
}

#[allow(clippy::type_complexity)]
fn update_statusbar(
    state: Res<EditorState>,
    mode: Res<CurrentMode>,
    modes: Res<Modes>,
    pending: Res<PendingKeys>,
    keymap: Res<ResolvedKeymap>,
    catalog: Res<ActionCatalog>,
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
    flash: Res<UnboundFlash>,
    time: Res<Time>,
) {
    for mut visibility in &mut bar {
        *visibility = if state.active { Visibility::Visible } else { Visibility::Hidden };
    }
    if !state.active {
        return;
    }

    let mode_def = modes.get(&mode.0);
    for mut text in &mut mode_text {
        let name = mode_def.map(|m| m.name.to_uppercase()).unwrap_or_else(|| "?".into());
        if text.0 != name {
            text.0 = name;
        }
    }
    for mut text in &mut hint_text {
        let hint = mode_def.map(|m| m.statusline_hint.to_string()).unwrap_or_default();
        if text.0 != hint {
            text.0 = hint;
        }
    }

    for (mut text, mut color) in &mut keys_text {
        if pending.0.is_empty() {
            if time.elapsed_secs() < flash.until {
                if text.0 != flash.text {
                    text.0 = flash.text.clone();
                    color.0 = style::color::TEXT_WARN;
                }
            } else if !text.0.is_empty() {
                text.0.clear();
            }
            continue;
        }
        color.0 = style::color::TEXT_KEYS;
        let contexts = active_contexts(&state, &mode);
        let continuations = which_key_continuations(&keymap, &contexts, &pending.0);
        let options: Vec<String> = continuations
            .iter()
            .map(|(chord, _, action)| {
                let name =
                    catalog.get(action).map(|d| d.name.as_ref()).unwrap_or(action.as_str());
                format!("{} {}", style::pretty_chord(chord), name)
            })
            .collect();
        text.0 = format!("{}    {}", style::pretty_chords(&pending.0), options.join("    "));
    }
}
