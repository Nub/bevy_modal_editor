//! The editor inside the game (spec §1) — only compiled with `--features editor`.
//!
//! M1 scope: the kernel (modes, resolver, keymaps), the command palette, and a real
//! status bar. Chrome follows the standing design bar (spec §7): themed surfaces,
//! never floating text over the render.

use bevy::feathers::palette;
use bevy::feathers::theme::{ThemeBackgroundColor, ThemedText};
use bevy::feathers::{dark_theme::create_dark_theme, theme::UiTheme, tokens, FeathersPlugins};
use bevy::prelude::*;
use bevy::ui::px;
use editor_core::prelude::*;

use crate::game::GameInputActive;

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
                (sync_game_input, update_statusbar).in_set(editor_core::EditorSet::Sync),
            );
        app.add_systems(Startup, spawn_statusbar);
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
            height: px(28),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(10),
            padding: UiRect::horizontal(px(10)),
        }
        ThemeBackgroundColor(tokens::PANE_HEADER_BG)
        GlobalZIndex(100)
        Visibility::Hidden
        Children [
            // mode chip: accent-filled rounded tag
            (
                StatusModeChip
                Node {
                    padding: UiRect::axes(px(8), px(2)),
                    align_items: AlignItems::Center,
                    border_radius: {BorderRadius::all(px(4))},
                }
                BackgroundColor({palette::ACCENT})
                Children [
                    (StatusModeText Text("NORMAL") TextColor({Color::srgb(0.05, 0.05, 0.08)}))
                ]
            ),
            (StatusHint Text("") ThemedText TextColor({Color::srgb(0.55, 0.57, 0.62)})),
            (Node { flex_grow: 1.0 }),
            (StatusKeys Text("") TextColor({Color::srgb(0.72, 0.74, 0.80)})),
        ]
    });
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
        &mut Text,
        (With<StatusKeys>, Without<StatusModeText>, Without<StatusHint>),
    >,
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

    for mut text in &mut keys_text {
        if pending.0.is_empty() {
            if !text.0.is_empty() {
                text.0.clear();
            }
            continue;
        }
        let typed: Vec<String> = pending.0.iter().map(|c| c.to_string()).collect();
        let contexts = active_contexts(&state, &mode);
        let continuations = which_key_continuations(&keymap, &contexts, &pending.0);
        let options: Vec<String> = continuations
            .iter()
            .map(|(chord, _, action)| {
                let name =
                    catalog.get(action).map(|d| d.name.as_ref()).unwrap_or(action.as_str());
                format!("{chord} → {name}")
            })
            .collect();
        text.0 = format!("{}    {}", typed.join(" "), options.join("    "));
    }
}
