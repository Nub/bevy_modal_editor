use bevy::prelude::*;

pub struct ToolbarPlugin;

impl Plugin for ToolbarPlugin {
    fn build(&self, _app: &mut App) {
        // Toolbar removed - using command palette (C key) instead
    }
}
