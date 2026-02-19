use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Marker on entities spawned from a prefab instance.
/// Tracks which prefab directory this entity came from and its unique instance ID.
#[derive(Component, Reflect, Serialize, Deserialize, Clone, Debug)]
#[reflect(Component, Serialize, Deserialize)]
pub struct PrefabInstance {
    /// Directory name under assets/prefabs/, e.g. "fireball"
    pub prefab_name: String,
    /// Unique per-instance identifier, e.g. "fireball_1"
    pub instance_id: String,
}

/// Marker on the root entity of a prefab instance (the group container).
#[derive(Component, Reflect, Serialize, Deserialize, Clone, Debug, Default)]
#[reflect(Component, Serialize, Deserialize)]
pub struct PrefabRoot;
