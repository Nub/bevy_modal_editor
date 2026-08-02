//! `game_framework` — the opinionated patterns a game follows (spec §3, §9).
//!
//! Never depends on any `editor_*` crate. The editor understands these states; games
//! and feature crates gate their systems on them instead of inventing parallel flags.
//!
//! M1 fleshes this out (sub-states, session/connection flow on lightyear semantics,
//! level-loading service, settings). This skeleton exists so `template_game` boots
//! through the canonical lifecycle from the first commit.

use bevy::prelude::*;

/// Top-level application lifecycle (spec §3). Sub-states (`Session`, connection state)
/// arrive in M1.
#[derive(States, Debug, Clone, PartialEq, Eq, Hash, Default)]
pub enum AppState {
    #[default]
    Boot,
    MainMenu,
    LoadingLevel,
    InGame,
}

pub struct GameFrameworkPlugin;

impl Plugin for GameFrameworkPlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<AppState>();
    }
}
