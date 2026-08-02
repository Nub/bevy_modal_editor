//! The reference game (spec §2). Every milestone's exit criteria are demonstrated
//! here. M1: boot through the `game_framework` lifecycle to a walkable graybox, with
//! the editor overlay available behind the `editor` feature.

use bevy::prelude::*;
use game_framework::GameFrameworkPlugin;
use template_game::game;

fn main() {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins)
        .add_plugins(GameFrameworkPlugin)
        .add_plugins(game::GamePlugin);

    #[cfg(feature = "editor")]
    app.add_plugins(template_game::editor_overlay::EditorOverlayPlugin);

    app.run();
}
