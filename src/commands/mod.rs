mod history;
mod operations;

pub use history::*;
pub use operations::*;

use bevy::prelude::*;

pub struct CommandsPlugin;

impl Plugin for CommandsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(HistoryPlugin).add_plugins(OperationsPlugin);
    }
}
