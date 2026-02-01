use avian3d::debug_render::PhysicsDebugPlugin;
use avian3d::prelude::{Physics, PhysicsPlugins};
use avian3d::schedule::PhysicsTime;
use bevy::prelude::*;
use bevy_egui::EguiPlugin;

use super::camera::EditorCameraPlugin;
use super::input::EditorInputPlugin;
use super::insert::InsertModePlugin;
use super::marks::CameraMarksPlugin;
use super::state::EditorStatePlugin;
use crate::commands::CommandsPlugin;
use crate::gizmos::EditorGizmosPlugin;
use crate::patterns::PatternsPlugin;
use crate::prefabs::PrefabsPlugin;
use crate::scene::ScenePlugin;
use crate::selection::SelectionPlugin;
use crate::ui::UiPlugin;

/// Main editor plugin that bundles all editor functionality
pub struct EditorPlugin;

impl Plugin for EditorPlugin {
    fn build(&self, app: &mut App) {
        app
            // Third-party plugins
            .add_plugins(EguiPlugin::default())
            .add_plugins(PhysicsPlugins::default())
            .add_plugins(PhysicsDebugPlugin)
            // Editor core
            .add_plugins(EditorStatePlugin)
            .add_plugins(EditorInputPlugin)
            .add_plugins(EditorCameraPlugin)
            .add_plugins(CameraMarksPlugin)
            .add_plugins(InsertModePlugin)
            // Editor systems
            .add_plugins(SelectionPlugin)
            .add_plugins(EditorGizmosPlugin)
            .add_plugins(ScenePlugin)
            .add_plugins(PrefabsPlugin)
            .add_plugins(CommandsPlugin)
            .add_plugins(PatternsPlugin)
            // UI
            .add_plugins(UiPlugin)
            // Setup
            .add_systems(Startup, (setup_editor_scene, pause_physics_on_startup));
    }
}

/// Setup initial editor scene with lighting
fn setup_editor_scene(mut commands: Commands) {
    // Ambient light (now a component in Bevy 0.18+)
    commands.spawn(AmbientLight {
        color: Color::WHITE,
        brightness: 300.0,
        affects_lightmapped_meshes: true,
    });
}

/// Pause physics simulation on startup by setting time scale to 0
fn pause_physics_on_startup(mut physics_time: ResMut<Time<Physics>>) {
    physics_time.set_relative_speed(0.0);
    info!("Physics simulation: PAUSED (default)");
}

