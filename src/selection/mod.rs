mod selection;

pub use selection::*;

use bevy::prelude::*;

pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(SelectionSystemPlugin);
    }
}
