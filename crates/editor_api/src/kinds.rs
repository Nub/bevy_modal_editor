//! Spawnable entity kinds (RFC §7): features declare what can be placed; the kernel
//! derives palette actions, insert previews, and spawn transactions from the registry
//! — adding a kind touches exactly one place (the v1 N-places-per-type killer).

use crate::ids::EntityKindId;
use bevy::prelude::*;
use bevy::reflect::PartialReflect;

#[derive(Clone)]
pub struct EntityKindDef {
    pub id: EntityKindId,
    pub display_name: &'static str,
    /// Semantic components for a new instance at `position` (runtime state like meshes
    /// derives via the game's regenerate observers — spec §5).
    pub components: fn(Vec3) -> Vec<Box<dyn PartialReflect>>,
}

/// Marker on the kernel's insert-mode ghost entity. Game-side regenerate observers
/// should render entities carrying this translucently and must never treat them as
/// scene content (no `SceneId` is present).
#[derive(Component, Default, Clone)]
pub struct InsertPreview;
