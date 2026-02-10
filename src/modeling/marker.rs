//! Serializable marker component for edited meshes.
//!
//! `EditMeshMarker` stores full vertex data so meshes survive save/load.
//! Same pattern as `StairsMarker` — runtime components (`Mesh3d`, `Collider`)
//! are regenerated from the marker on restore.

use avian3d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::edit_mesh::EditMesh;

/// Serializable component storing the full vertex data of an edited mesh.
///
/// When present, this governs the entity's mesh — `regenerate_runtime_components`
/// rebuilds `Mesh3d` and `Collider::trimesh()` from this data.
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct EditMeshMarker {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

impl EditMeshMarker {
    /// Create a marker from an `EditMesh`.
    pub fn from_edit_mesh(mesh: &EditMesh) -> Self {
        Self {
            positions: mesh.positions.iter().map(|p| [p.x, p.y, p.z]).collect(),
            normals: mesh.normals.iter().map(|n| [n.x, n.y, n.z]).collect(),
            uvs: mesh.uvs.iter().map(|u| [u.x, u.y]).collect(),
            indices: mesh.triangles.iter().flat_map(|t| t.iter().copied()).collect(),
        }
    }

    /// Convert back to an `EditMesh`.
    pub fn to_edit_mesh(&self) -> EditMesh {
        EditMesh {
            positions: self.positions.iter().map(|p| Vec3::from(*p)).collect(),
            normals: self.normals.iter().map(|n| Vec3::from(*n)).collect(),
            uvs: self.uvs.iter().map(|u| Vec2::from(*u)).collect(),
            triangles: self
                .indices
                .chunks(3)
                .map(|c| [c[0], c[1], c[2]])
                .collect(),
        }
    }

    /// Build a Bevy `Mesh` from this marker's data.
    pub fn to_bevy_mesh(&self) -> Mesh {
        self.to_edit_mesh().to_bevy_mesh()
    }

    /// Build a trimesh `Collider` from this marker's data.
    pub fn to_collider(&self) -> Collider {
        let vertices: Vec<Vec3> = self.positions.iter().map(|p| Vec3::from(*p)).collect();
        let indices: Vec<[u32; 3]> = self
            .indices
            .chunks(3)
            .map(|c| [c[0], c[1], c[2]])
            .collect();
        Collider::trimesh(vertices, indices)
    }
}

/// Regenerate `Mesh3d` and `Collider` for entities with `EditMeshMarker` that
/// are missing their runtime mesh (e.g. after scene load).
pub fn regenerate_edit_meshes(world: &mut World) {
    let mut to_update: Vec<(Entity, EditMeshMarker)> = Vec::new();
    {
        let mut query =
            world.query_filtered::<(Entity, &EditMeshMarker), Without<Mesh3d>>();
        for (entity, marker) in query.iter(world) {
            to_update.push((entity, marker.clone()));
        }
    }

    for (entity, marker) in to_update {
        let mesh = marker.to_bevy_mesh();
        let collider = marker.to_collider();
        let mesh_handle = world.resource_mut::<Assets<Mesh>>().add(mesh);
        if let Ok(mut entity_mut) = world.get_entity_mut(entity) {
            entity_mut.insert((Mesh3d(mesh_handle), collider));
        }
    }
}
