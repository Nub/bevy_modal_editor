//! The editor inside the game (spec §1) — only compiled with `--features editor`.
//!
//! M1 scope: the kernel (modes, resolver, keymaps) plus a minimal overlay statusline.
//! F12 (core.toggle-editor, rebindable like everything) switches ownership of input
//! between game and editor. The panel shell/palette UI lands on top of this.

use bevy::feathers::{dark_theme::create_dark_theme, theme::UiTheme, FeathersPlugins};
use bevy::prelude::*;
use editor_core::prelude::*;

use crate::game::GameInputActive;

#[derive(Component)]
struct Statusline;

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
            (sync_game_input, update_statusline).in_set(editor_core::EditorSet::Sync),
        );
        app.add_systems(Startup, spawn_statusline);
    }
}

fn spawn_statusline(mut commands: Commands) {
    commands.spawn((
        Statusline,
        Text::new(""),
        Node {
            position_type: PositionType::Absolute,
            left: bevy::ui::px(8),
            bottom: bevy::ui::px(6),
            ..default()
        },
        GlobalZIndex(100),
        Visibility::Hidden,
    ));
}

/// The editor owns input while active; the game module knows nothing about the
/// editor — it just honors `GameInputActive`.
fn sync_game_input(state: Res<EditorState>, mut game_input: ResMut<GameInputActive>) {
    let game_owns = !state.active;
    if game_input.0 != game_owns {
        game_input.0 = game_owns;
    }
}

fn update_statusline(
    state: Res<EditorState>,
    mode: Res<CurrentMode>,
    modes: Res<Modes>,
    pending: Res<PendingKeys>,
    keymap: Res<ResolvedKeymap>,
    mut status: Query<(&mut Text, &mut Visibility), With<Statusline>>,
) {
    for (mut text, mut visibility) in &mut status {
        if !state.active {
            *visibility = Visibility::Hidden;
            continue;
        }
        *visibility = Visibility::Visible;

        let mode_name = modes.get(&mode.0).map(|m| m.name.as_ref()).unwrap_or("?");
        let hint = modes
            .get(&mode.0)
            .map(|m| m.statusline_hint.as_ref())
            .unwrap_or("");

        let mut line = format!("-- {} --  {}", mode_name.to_uppercase(), hint);
        if !pending.0.is_empty() {
            let keys: Vec<String> = pending.0.iter().map(|c| c.to_string()).collect();
            let contexts = editor_core::prelude::active_contexts(&state, &mode);
            let conts = which_key_continuations(&keymap, &contexts, &pending.0);
            let options: Vec<String> = conts
                .iter()
                .map(|(chord, _, action)| format!("{chord}:{action}"))
                .collect();
            line = format!("{line}   [{}]  {}", keys.join(" "), options.join("  "));
        }
        text.0 = line;
    }
}
