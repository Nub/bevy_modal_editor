//! The reference game (spec §2). Every milestone's exit criteria are demonstrated here.
//!
//! M0 state: boots through `game_framework`'s lifecycle to a window. M1 makes it
//! walkable and adds the editor overlay behind the `editor` feature.

use bevy::prelude::*;
use game_framework::{AppState, GameFrameworkPlugin};

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_plugins(GameFrameworkPlugin)
        .add_systems(Startup, boot)
        .run();
}

fn boot(mut next: ResMut<NextState<AppState>>) {
    // M0: no menu yet — go straight to MainMenu state to prove the lifecycle wiring.
    next.set(AppState::MainMenu);
}
