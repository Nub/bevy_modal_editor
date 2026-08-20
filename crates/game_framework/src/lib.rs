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

/// Build a primitive mesh WITH tangents.
///
/// Bevy compiles the whole normal-mapping branch out unless a mesh's vertex
/// layout carries `ATTRIBUTE_TANGENT` (the PBR shader gates it behind
/// `#ifdef VERTEX_TANGENTS`), and no primitive builder emits one. A normal map
/// on a greybox cube is therefore discarded in silence — no warning, no error,
/// just a surface that never changes. glTF meshes arrive with tangents from the
/// importer; anything a game generates has to ask.
pub fn primitive_mesh(shape: impl Into<Mesh>) -> Mesh {
    let mut mesh = shape.into();
    if let Err(error) = mesh.generate_tangents() {
        // Not fatal: the surface still shades, it just cannot show a normal map.
        warn!("no tangents for a primitive mesh; normal maps will not show: {error}");
    }
    mesh
}
