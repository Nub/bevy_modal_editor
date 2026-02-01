use avian3d::debug_render::PhysicsDebugPlugin;
use avian3d::prelude::PhysicsPlugins;
use bevy::prelude::*;
use bevy_egui::EguiPlugin;

use super::camera::EditorCameraPlugin;
use super::input::EditorInputPlugin;
use super::marks::CameraMarksPlugin;
use super::state::EditorStatePlugin;
use crate::commands::CommandsPlugin;
use crate::gizmos::EditorGizmosPlugin;
use crate::patterns::PatternsPlugin;
use crate::prefabs::PrefabsPlugin;
use crate::scene::{LoadSceneEvent, ScenePlugin};
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
            .add_systems(Startup, setup_editor_scene)
            .add_systems(PostStartup, load_default_scene);
    }
}

/// Setup initial editor scene with lighting
fn setup_editor_scene(mut commands: Commands) {
    // Directional light
    commands.spawn((
        DirectionalLight {
            illuminance: 10000.0,
            shadows_enabled: true,
            ..default()
        },
        Transform::from_xyz(4.0, 8.0, 4.0).looking_at(Vec3::ZERO, Vec3::Y),
    ));

    // Ambient light (now a component in Bevy 0.18+)
    commands.spawn(AmbientLight {
        color: Color::WHITE,
        brightness: 300.0,
        affects_lightmapped_meshes: true,
    });
}

/// Load the default scene on startup
fn load_default_scene(mut load_events: MessageWriter<LoadSceneEvent>) {
    // Load the test scene
    load_events.write(LoadSceneEvent {
        path: "assets/test_scene.ron".to_string(),
    });

    info!("Loading test scene");
}
