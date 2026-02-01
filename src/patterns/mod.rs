mod circular;
mod linear;

pub use circular::*;
pub use linear::*;

use bevy::prelude::*;

pub struct PatternsPlugin;

impl Plugin for PatternsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(LinearPatternPlugin)
            .add_plugins(CircularPatternPlugin);
    }
}
