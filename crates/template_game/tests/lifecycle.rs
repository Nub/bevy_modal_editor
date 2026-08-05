//! M1 acceptance A7: the game boots headless through the framework lifecycle
//! Boot → MainMenu → LoadingLevel → InGame, and the level actually exists.

use bevy::asset::AssetPlugin;
use bevy::input::mouse::AccumulatedMouseMotion;
use bevy::prelude::*;
use bevy::state::app::StatesPlugin;
use game_framework::{AppState, GameFrameworkPlugin};
use template_game::game::{GamePlugin, Player};

fn headless_game() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, StatesPlugin, AssetPlugin::default()));
    app.init_asset::<Mesh>();
    app.init_asset::<StandardMaterial>();
    app.init_resource::<AccumulatedMouseMotion>();
    app.init_resource::<ButtonInput<KeyCode>>();
    app.add_plugins((GameFrameworkPlugin, GamePlugin));
    app
}

fn state(app: &App) -> AppState {
    app.world().resource::<State<AppState>>().get().clone()
}

#[test]
fn boots_to_ingame_through_lifecycle() {
    let mut app = headless_game();

    // Boot -> MainMenu (startup system).
    app.update();
    app.update();
    assert_eq!(state(&app), AppState::MainMenu);

    // Enter starts the game: MainMenu -> LoadingLevel -> InGame.
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(KeyCode::Enter);
    app.update();
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .clear();
    app.update();
    app.update();
    assert_eq!(state(&app), AppState::InGame);

    // The graybox exists: a player camera and level geometry.
    let world = app.world_mut();
    assert_eq!(
        world.query::<&Player>().iter(world).count(),
        1,
        "one player"
    );
    let mesh_count = world.query::<&Mesh3d>().iter(world).count();
    assert!(mesh_count >= 5, "ground + boxes spawned (got {mesh_count})");
}
