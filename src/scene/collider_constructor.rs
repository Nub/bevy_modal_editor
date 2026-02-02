use avian3d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// The type of collider to generate from meshes
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Reflect, Serialize, Deserialize)]
#[reflect(Default)]
pub enum ColliderType {
    /// Generate a convex hull collider (good for most objects)
    #[default]
    ConvexHull,
    /// Generate a trimesh collider (accurate but only works for static objects)
    Trimesh,
    /// Generate a convex decomposition (good for complex dynamic objects)
    ConvexDecomposition,
}

impl ColliderType {
    /// Convert to Avian3D's ColliderConstructor
    fn to_constructor(&self) -> ColliderConstructor {
        match self {
            ColliderType::ConvexHull => ColliderConstructor::ConvexHullFromMesh,
            ColliderType::Trimesh => ColliderConstructor::TrimeshFromMesh,
            ColliderType::ConvexDecomposition => ColliderConstructor::ConvexDecompositionFromMesh,
        }
    }
}

/// Component that recursively adds colliders to all descendant entities with meshes.
///
/// When added to an entity, this component will walk through all children (recursively)
/// and add a `ColliderConstructor` to any entity that has a `Mesh3d` component.
///
/// This is useful for GLTF models where you want automatic collision detection.
#[derive(Component, Reflect, Default, Clone, Serialize, Deserialize)]
#[reflect(Component, Default)]
pub struct RecursiveColliderConstructor {
    /// The type of collider to generate
    pub collider_type: ColliderType,
}

/// Marker component to track which entities had colliders added by RecursiveColliderConstructor
#[derive(Component)]
struct GeneratedCollider;

/// Tracks the last applied collider type for change detection
#[derive(Component)]
struct AppliedColliderType(ColliderType);

pub struct ColliderConstructorPlugin;

impl Plugin for ColliderConstructorPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<RecursiveColliderConstructor>()
            .register_type::<ColliderType>()
            .add_systems(Update, (
                apply_recursive_collider_constructor,
                handle_constructor_changed,
                cleanup_on_remove,
            ));
    }
}

/// System that processes RecursiveColliderConstructor components
fn apply_recursive_collider_constructor(
    mut commands: Commands,
    query: Query<(Entity, &RecursiveColliderConstructor), Without<AppliedColliderType>>,
    children_query: Query<&Children>,
    mesh_query: Query<Entity, With<Mesh3d>>,
    collider_query: Query<(), With<Collider>>,
    generated_query: Query<(), With<GeneratedCollider>>,
) {
    for (entity, constructor) in query.iter() {
        let collider_constructor = constructor.collider_type.to_constructor();

        // Recursively process all descendants
        let count = process_descendants(
            &mut commands,
            entity,
            &children_query,
            &mesh_query,
            &collider_query,
            &generated_query,
            &collider_constructor,
        );

        // Mark as processed with the applied type
        commands.entity(entity).insert(AppliedColliderType(constructor.collider_type));

        if count > 0 {
            info!("Applied {:?} colliders to {} mesh entities", constructor.collider_type, count);
        }
    }
}

/// Recursively walk through children and add ColliderConstructor to entities with meshes
fn process_descendants(
    commands: &mut Commands,
    entity: Entity,
    children_query: &Query<&Children>,
    mesh_query: &Query<Entity, With<Mesh3d>>,
    collider_query: &Query<(), With<Collider>>,
    generated_query: &Query<(), With<GeneratedCollider>>,
    collider_constructor: &ColliderConstructor,
) -> usize {
    let mut count = 0;

    // Check if this entity has a mesh
    if mesh_query.get(entity).is_ok() {
        // Only add if no collider exists, or if we previously generated one (will be replaced)
        let has_collider = collider_query.get(entity).is_ok();
        let was_generated = generated_query.get(entity).is_ok();

        if !has_collider || was_generated {
            commands.entity(entity).insert((
                collider_constructor.clone(),
                GeneratedCollider,
            ));
            count += 1;
        }
    }

    // Process children recursively
    if let Ok(children) = children_query.get(entity) {
        for child in children.iter() {
            count += process_descendants(
                commands,
                child,
                children_query,
                mesh_query,
                collider_query,
                generated_query,
                collider_constructor,
            );
        }
    }

    count
}

/// System to handle when the collider type changes
fn handle_constructor_changed(
    mut commands: Commands,
    changed: Query<(Entity, &RecursiveColliderConstructor, &AppliedColliderType), Changed<RecursiveColliderConstructor>>,
    children_query: Query<&Children>,
    generated_query: Query<Entity, With<GeneratedCollider>>,
) {
    for (entity, constructor, applied) in changed.iter() {
        // Only re-process if the collider type actually changed
        if constructor.collider_type != applied.0 {
            info!("Collider type changed from {:?} to {:?}, regenerating", applied.0, constructor.collider_type);

            // Remove existing generated colliders from descendants
            remove_generated_colliders(&mut commands, entity, &children_query, &generated_query);

            // Remove the applied marker so it gets re-processed
            commands.entity(entity).remove::<AppliedColliderType>();
        }
    }
}

/// Clean up generated colliders when RecursiveColliderConstructor is removed
fn cleanup_on_remove(
    mut commands: Commands,
    mut removed: RemovedComponents<RecursiveColliderConstructor>,
    children_query: Query<&Children>,
    generated_query: Query<Entity, With<GeneratedCollider>>,
    applied_query: Query<Entity, With<AppliedColliderType>>,
) {
    for entity in removed.read() {
        // Remove generated colliders from descendants
        remove_generated_colliders(&mut commands, entity, &children_query, &generated_query);

        // Remove the applied marker
        if applied_query.get(entity).is_ok() {
            commands.entity(entity).remove::<AppliedColliderType>();
        }
    }
}

/// Recursively remove generated colliders from an entity and its descendants
fn remove_generated_colliders(
    commands: &mut Commands,
    entity: Entity,
    children_query: &Query<&Children>,
    generated_query: &Query<Entity, With<GeneratedCollider>>,
) {
    // Remove collider if it was generated by us
    if generated_query.get(entity).is_ok() {
        commands.entity(entity).remove::<Collider>();
        commands.entity(entity).remove::<GeneratedCollider>();
    }

    // Process children recursively
    if let Ok(children) = children_query.get(entity) {
        for child in children.iter() {
            remove_generated_colliders(commands, child, children_query, generated_query);
        }
    }
}
