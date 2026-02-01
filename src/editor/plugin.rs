use avian3d::prelude::*;
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
            .add_systems(Startup, setup_editor_scene);
    }
}

/// Setup initial editor scene with lighting and ground plane
fn setup_editor_scene(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
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

    // Ground plane (visual reference, not part of scene save)
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(20.0, 20.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.3, 0.3, 0.3),
            ..default()
        })),
        Transform::from_xyz(0.0, -0.01, 0.0),
    ));

    // Grid lines using gizmos will be drawn in the gizmos module
}
