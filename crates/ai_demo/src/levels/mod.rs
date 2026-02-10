//! Level system: auto-generates and loads the arena level on startup.

pub mod arena;
pub mod level_gen;

use bevy::prelude::*;
use bevy_modal_editor::scene::{LoadSceneEvent, SceneFile};
use bevy_modal_editor::SceneEntity;

pub struct LevelsPlugin;

impl Plugin for LevelsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(PreStartup, level_gen::generate_and_load_level)
            .add_systems(Update, auto_load_arena);
    }
}

/// Auto-load the arena level on the first frame if no scene is loaded.
fn auto_load_arena(
    mut ran: Local<bool>,
    mut load_events: MessageWriter<LoadSceneEvent>,
    mut scene_file: ResMut<SceneFile>,
    existing: Query<Entity, With<SceneEntity>>,
) {
    if *ran {
        return;
    }
    *ran = true;

    if existing.iter().count() == 0 && scene_file.path.is_none() {
        let path = level_gen::level_path("arena.scn.ron");
        load_events.write(LoadSceneEvent { path: path.clone() });
        scene_file.path = Some(path);
        scene_file.clear_modified();
    }
}
