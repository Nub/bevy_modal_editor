mod prefab;
mod registry;
mod spawn;

pub use prefab::*;
pub use registry::*;
pub use spawn::*;

use bevy::prelude::*;

pub struct PrefabsPlugin;

impl Plugin for PrefabsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PrefabRegistryPlugin)
            .add_plugins(PrefabSpawnPlugin);
    }
}
