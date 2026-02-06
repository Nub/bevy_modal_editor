//! Level system: level registry, auto-generation, and auto-loading.

pub mod level_gen;
mod locomotion_arena;

use bevy::prelude::*;
use bevy_editor_game::GameState;
use bevy_modal_editor::SceneEntity;
use bevy_modal_editor::scene::{LoadSceneEvent, SceneFile};

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

/// Description of a single level.
#[derive(Clone)]
pub struct LevelInfo {
    pub name: &'static str,
    pub description: &'static str,
    pub filename: &'static str,
    pub builder: fn(&mut World),
    pub index: usize,
}

/// Registry of all available levels.
#[derive(Resource, Clone)]
pub struct LevelRegistry {
    pub levels: Vec<LevelInfo>,
}

impl Default for LevelRegistry {
    fn default() -> Self {
        Self {
            levels: vec![LevelInfo {
                name: "Locomotion Arena",
                description:
                    "Large interconnected arena testing all locomotion features with 10 hidden coins.",
                filename: "locomotion_arena.scn.ron",
                builder: locomotion_arena::build_locomotion_arena,
                index: 0,
            }],
        }
    }
}

// ---------------------------------------------------------------------------
// Plugin
// ---------------------------------------------------------------------------

pub struct LevelsPlugin;

impl Plugin for LevelsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(LevelRegistry::default())
            .add_systems(PreStartup, level_gen::generate_missing_level_files)
            .add_systems(Update, auto_load_level);
    }
}

// ---------------------------------------------------------------------------
// Systems
// ---------------------------------------------------------------------------

/// Auto-load the level on startup if no scene is loaded.
fn auto_load_level(
    mut ran: Local<bool>,
    registry: Res<LevelRegistry>,
    existing: Query<Entity, With<SceneEntity>>,
    mut scene_file: ResMut<SceneFile>,
    game_state: Res<State<GameState>>,
    mut load_events: MessageWriter<LoadSceneEvent>,
) {
    if *ran {
        return;
    }

    // Only auto-load if in Editing state, no scene entities, and no file loaded
    if *game_state.get() != GameState::Editing {
        return;
    }
    if existing.iter().count() > 0 || scene_file.path.is_some() {
        *ran = true;
        return;
    }

    *ran = true;

    if let Some(level) = registry.levels.first() {
        let path = level_gen::level_path(level.filename);
        info!("Auto-loading level: {} ({})", level.name, path);
        load_events.write(LoadSceneEvent { path: path.clone() });
        scene_file.path = Some(path);
        scene_file.clear_modified();
    }
}
