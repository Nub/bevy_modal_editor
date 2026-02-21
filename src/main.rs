//! Main binary for running the editor standalone.
//!
//! For using the editor as a library in your own project,
//! see the examples directory.

use bevy::prelude::*;
use bevy_modal_editor::{recommended_image_plugin, EditorPlugin};

fn main() {
    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Bevy Avian3D Editor".to_string(),
                        ..default()
                    }),
                    ..default()
                })
                .set(recommended_image_plugin()),
        )
        .add_plugins(EditorPlugin::default())
        .run();
}
