use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::scene::{PrimitiveShape, SerializedRigidBody, SerializedTransform};

/// A prefab is a reusable template of entities
#[derive(Asset, TypePath, Serialize, Deserialize, Clone)]
pub struct Prefab {
    pub name: String,
    pub entities: Vec<PrefabEntity>,
}

/// A single entity within a prefab
#[derive(Serialize, Deserialize, Clone)]
pub struct PrefabEntity {
    pub name: String,
    pub transform: SerializedTransform,
    pub primitive: Option<PrimitiveShape>,
    pub rigid_body: Option<SerializedRigidBody>,
    pub children: Vec<PrefabEntity>,
}

impl Default for Prefab {
    fn default() -> Self {
        Self {
            name: "New Prefab".to_string(),
            entities: Vec::new(),
        }
    }
}

/// Component to mark an entity as a prefab instance
#[derive(Component)]
pub struct PrefabInstance {
    pub prefab_name: String,
}

/// Component to mark the root of a prefab instance
#[derive(Component)]
pub struct PrefabRoot;
