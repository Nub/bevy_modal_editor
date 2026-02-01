//! Simple example showing how to use the Bevy Avian3D Editor plugin.
//!
//! Run with: `cargo run --example simple`
//!
//! This creates an empty editor where you can:
//! - Press 'I' to enter Insert mode and add primitives
//! - Press 'E' or 'V' to enter Edit mode and transform objects
//! - Press 'O' to enter Object Inspector mode
//! - Press 'H' to enter Hierarchy mode
//! - Press '?' for help

use bevy::prelude::*;
use bevy_modal_editor::EditorPlugin;

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bevy Avian3D Editor - Simple Example".to_string(),
                ..default()
            }),
            ..default()
        }))
        .add_plugins(EditorPlugin::default())
        .run();
}
