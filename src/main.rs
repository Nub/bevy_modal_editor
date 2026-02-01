use bevy::prelude::*;
use bevy_avian3d_editor::EditorPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy Avian3D Editor".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EditorPlugin)
        .run();
}
