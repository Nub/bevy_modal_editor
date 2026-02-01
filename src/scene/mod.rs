mod primitives;
mod serialization;

pub use primitives::*;
pub use serialization::*;

use bevy::prelude::*;

/// Marker component for entities that are part of the editable scene
#[derive(Component, Default)]
pub struct SceneEntity;

pub struct ScenePlugin;

impl Plugin for ScenePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(PrimitivesPlugin).add_plugins(SerializationPlugin);
    }
}
