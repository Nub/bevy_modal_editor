//! # bevy_procedural
//!
//! A Bevy plugin for procedural object placement on meshes and splines.
//!
//! ## Quick Start
//!
//! Add `ProceduralPlacer` to any entity with a `Mesh3d` or `Spline` component:
//!
//! ```ignore
//! use bevy::prelude::*;
//! use bevy_procedural::prelude::*;
//!
//! fn setup(mut commands: Commands) {
//!     // Create template entities (will be hidden automatically)
//!     let tree = commands.spawn((Mesh3d::default(), PlacementTemplate)).id();
//!     let rock = commands.spawn((Mesh3d::default(), PlacementTemplate)).id();
//!
//!     // Add ProceduralPlacer to any mesh - it samples its own surface
//!     commands.spawn((
//!         Mesh3d::default(), // terrain mesh
//!         ProceduralPlacer::new(vec![
//!             WeightedTemplate::new(tree, 1.0),
//!             WeightedTemplate::new(rock, 0.5),
//!         ])
//!         .with_count(100)
//!         .with_mode(SamplingMode::Random { seed: Some(42) })
//!         .with_orientation(PlacementOrientation::AlignToSurface),
//!     ));
//! }
//! ```

pub mod placer;
pub mod projection;
pub mod sampling;

#[cfg(feature = "spline")]
mod spline_sampling;

use avian3d::prelude::*;
use bevy::prelude::*;

pub use placer::{
    PlacementOrientation, ProceduralEntity, ProceduralPlacer, ProceduralTemplate, SamplingMode,
    SurfaceProjection, WeightedTemplate,
};
// Deprecated re-exports for backwards compatibility
#[allow(deprecated)]
pub use placer::{PlacedInstance, PlacementTemplate};

pub use projection::{project_to_surface, ProjectionResult};
pub use sampling::{Sample, SampleOrientation, Sampling};

/// Convenient re-exports of commonly used types.
pub mod prelude {
    pub use crate::placer::{
        PlacementOrientation, ProceduralEntity, ProceduralPlacer, ProceduralTemplate, SamplingMode,
        SurfaceProjection, WeightedTemplate,
    };
    pub use crate::projection::{project_to_surface, ProjectionResult};
    pub use crate::sampling::{Sample, SampleOrientation, Sampling};
    pub use crate::ProceduralPlugin;
}

/// Plugin that provides procedural placement functionality.
pub struct ProceduralPlugin;

impl Plugin for ProceduralPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<ProceduralPlacer>()
            .register_type::<SamplingMode>()
            .register_type::<PlacementOrientation>()
            .register_type::<SurfaceProjection>()
            .register_type::<WeightedTemplate>()
            .register_type::<ProceduralEntity>()
            .register_type::<ProceduralTemplate>()
            .add_systems(
                Update,
                (hide_template_entities, update_placements, cleanup_placements).chain(),
            )
            .add_systems(PostUpdate, project_placed_instances);
    }
}

/// System to hide template entities.
fn hide_template_entities(mut query: Query<&mut Visibility, Added<ProceduralTemplate>>) {
    for mut visibility in &mut query {
        *visibility = Visibility::Hidden;
    }
}

/// System to update placements when placer or source changes.
fn update_placements(
    world: &mut World,
) {
    // Collect placers that need updating.
    // Triggers on: ProceduralPlacer change, Mesh3d change, or GlobalTransform change.
    let mesh_placers: Vec<_> = world
        .query_filtered::<(Entity, &ProceduralPlacer, &Mesh3d, &GlobalTransform), Or<(Changed<ProceduralPlacer>, Added<ProceduralPlacer>, Changed<Mesh3d>, Changed<GlobalTransform>)>>()
        .iter(world)
        .filter(|(_, p, _, _)| p.enabled)
        .map(|(e, p, m, t)| (e, p.clone(), m.0.clone(), *t))
        .collect();

    // Collect spline-based placers.
    // Triggers on: ProceduralPlacer change, Spline change, or GlobalTransform change.
    #[cfg(feature = "spline")]
    let spline_placers: Vec<_> = world
        .query_filtered::<(Entity, &ProceduralPlacer, &bevy_spline_3d::spline::Spline, &GlobalTransform), (Or<(Changed<ProceduralPlacer>, Added<ProceduralPlacer>, Changed<bevy_spline_3d::spline::Spline>, Changed<GlobalTransform>)>, Without<Mesh3d>)>()
        .iter(world)
        .filter(|(_, p, _, _)| p.enabled)
        .map(|(e, p, s, t)| (e, p.clone(), s.clone(), *t))
        .collect();

    // Collect existing instances to remove
    let instances_to_remove: Vec<_> = world
        .query::<(Entity, &ProceduralEntity)>()
        .iter(world)
        .map(|(e, i)| (e, i.placer))
        .collect();

    // First, sample all meshes while we have immutable access
    let mesh_samples: Vec<_> = {
        let meshes = world.resource::<Assets<Mesh>>();
        mesh_placers.iter().map(|(placer_entity, placer, mesh_handle, global_transform)| {
            let samples = meshes.get(mesh_handle.id())
                .map(|mesh| mesh.sample(placer.count, placer.mode.is_uniform(), placer.mode.seed()))
                .unwrap_or_default();
            (*placer_entity, placer.clone(), samples, *global_transform)
        }).collect()
    };

    // Process mesh-based placers
    for (placer_entity, placer, samples, global_transform) in mesh_samples {
        // Remove existing instances for this placer
        for (instance_entity, instance_placer) in &instances_to_remove {
            if *instance_placer == placer_entity {
                world.despawn(*instance_entity);
            }
        }

        if samples.is_empty() {
            continue;
        }

        spawn_instances_world(
            world,
            placer_entity,
            &placer,
            &samples,
            &global_transform,
        );
    }

    // Process spline-based placers
    #[cfg(feature = "spline")]
    for (placer_entity, placer, spline, global_transform) in &spline_placers {
        // Remove existing instances for this placer
        for (instance_entity, instance_placer) in &instances_to_remove {
            if instance_placer == placer_entity {
                world.despawn(*instance_entity);
            }
        }

        let samples = spline_sampling::sample_spline(&spline, placer.count, placer.mode.is_uniform(), placer.mode.seed());

        spawn_instances_world(
            world,
            *placer_entity,
            &placer,
            &samples,
            global_transform,
        );
    }
}

/// Spawn instances from samples using entity cloning.
fn spawn_instances_world(
    world: &mut World,
    placer_entity: Entity,
    placer: &ProceduralPlacer,
    samples: &[Sample],
    source_transform: &GlobalTransform,
) {
    if placer.templates.is_empty() {
        return;
    }

    // Build weighted selection
    let total_weight: f32 = placer.templates.iter().map(|t| t.weight).sum();
    if total_weight <= 0.0 {
        return;
    }

    let mut rng = fastrand::Rng::with_seed(placer.mode.seed().unwrap_or(0));

    // Collect spawn data first to avoid borrow issues
    let spawn_data: Vec<_> = samples.iter().enumerate().filter_map(|(index, sample)| {
        // Select template based on weight
        let template = select_weighted_template(&placer.templates, total_weight, &mut rng);

        // Get template transform
        let template_transform = world.get::<Transform>(template.entity).copied()?;

        // Convert sample position from local to world space
        let world_position = source_transform.transform_point(sample.position);

        // Calculate rotation based on orientation mode
        let rotation = calculate_instance_rotation(
            &placer.orientation,
            &sample.orientation,
            template_transform.rotation,
            index,
            placer.mode.seed(),
        );

        // Apply offset in local space
        let offset_position = world_position + rotation * placer.offset;

        Some((template.entity, offset_position, rotation, template_transform.scale, index))
    }).collect();

    // Now spawn instances
    for (template_entity, position, rotation, scale, index) in spawn_data {
        // Guard against despawned templates (can happen after undo/scene reload)
        if world.get_entity(template_entity).is_err() {
            warn!("Procedural template entity {template_entity:?} no longer exists, skipping");
            continue;
        }

        // Clone the template entity, denying ChildOf to prevent hierarchy
        // contamination — instance positions are in world space, so inheriting
        // a parent transform would double-apply it.
        let cloned = world
            .entity_mut(template_entity)
            .clone_and_spawn_with_opt_out(|builder| {
                builder.deny::<ChildOf>();
            });

        // Override transform with calculated position/rotation
        if let Some(mut transform) = world.get_mut::<Transform>(cloned) {
            transform.translation = position;
            transform.rotation = rotation;
            transform.scale = scale;
        }

        // Add the procedural entity marker and remove template marker
        world.entity_mut(cloned)
            .insert(ProceduralEntity {
                placer: placer_entity,
                template: template_entity,
                index,
            })
            .remove::<ProceduralTemplate>();

        // Make visible (templates are hidden)
        if let Some(mut visibility) = world.get_mut::<Visibility>(cloned) {
            *visibility = Visibility::Inherited;
        }
    }
}

/// Select a template based on weights.
fn select_weighted_template<'a>(
    templates: &'a [WeightedTemplate],
    total_weight: f32,
    rng: &mut fastrand::Rng,
) -> &'a WeightedTemplate {
    let r = rng.f32() * total_weight;
    let mut cumulative = 0.0;

    for template in templates {
        cumulative += template.weight;
        if r <= cumulative {
            return template;
        }
    }

    // Fallback to last template
    templates.last().unwrap()
}

/// Calculate rotation for an instance based on orientation mode.
fn calculate_instance_rotation(
    orientation: &PlacementOrientation,
    sample_orientation: &SampleOrientation,
    template_rotation: Quat,
    index: usize,
    seed: Option<u64>,
) -> Quat {
    match orientation {
        PlacementOrientation::Identity => template_rotation,

        PlacementOrientation::AlignToTangent { up } => {
            if let Some(tangent) = sample_orientation.tangent {
                let tangent = tangent.normalize_or_zero();
                if tangent.length_squared() < 0.001 {
                    return template_rotation;
                }

                let up_vec = sample_orientation.up.unwrap_or(*up).normalize_or_zero();
                let right = up_vec.cross(tangent).normalize_or_zero();
                if right.length_squared() < 0.001 {
                    return template_rotation;
                }

                let corrected_up = tangent.cross(right).normalize();
                Quat::from_mat3(&Mat3::from_cols(right, corrected_up, tangent))
            } else {
                template_rotation
            }
        }

        PlacementOrientation::AlignToSurface => {
            // Surface alignment is handled in projection system
            template_rotation
        }

        PlacementOrientation::RandomYaw => {
            let mut rng = fastrand::Rng::with_seed(seed.unwrap_or(0) + index as u64);
            let angle = rng.f32() * std::f32::consts::TAU;
            template_rotation * Quat::from_rotation_y(angle)
        }

        PlacementOrientation::RandomFull => {
            let mut rng = fastrand::Rng::with_seed(seed.unwrap_or(0) + index as u64);
            let u1 = rng.f32();
            let u2 = rng.f32() * std::f32::consts::TAU;
            let u3 = rng.f32() * std::f32::consts::TAU;

            let sqrt_u1 = u1.sqrt();
            let sqrt_1_u1 = (1.0 - u1).sqrt();

            template_rotation
                * Quat::from_xyzw(
                    sqrt_1_u1 * u2.sin(),
                    sqrt_1_u1 * u2.cos(),
                    sqrt_u1 * u3.sin(),
                    sqrt_u1 * u3.cos(),
                )
        }
    }
}

/// System to cleanup instances when placer is removed.
fn cleanup_placements(
    mut commands: Commands,
    mut removed_placers: RemovedComponents<ProceduralPlacer>,
    instances: Query<(Entity, &ProceduralEntity)>,
) {
    for removed_placer in removed_placers.read() {
        for (instance_entity, instance) in &instances {
            if instance.placer == removed_placer {
                commands.entity(instance_entity).despawn();
            }
        }
    }
}

/// System to project placed instances onto surfaces.
fn project_placed_instances(
    spatial_query: SpatialQuery,
    placers: Query<(&ProceduralPlacer, &GlobalTransform)>,
    mut instances: Query<(Entity, &ProceduralEntity, &mut Transform)>,
    colliders: Query<&Collider>,
) {
    use std::collections::HashMap;

    // First, collect all instance entities grouped by placer
    let mut instances_by_placer: HashMap<Entity, Vec<Entity>> = HashMap::new();
    for (entity, instance, _) in instances.iter() {
        instances_by_placer
            .entry(instance.placer)
            .or_default()
            .push(entity);
    }

    for (instance_entity, instance, mut transform) in &mut instances {
        let Ok((placer, source_transform)) = placers.get(instance.placer) else {
            continue;
        };

        if !placer.projection.enabled {
            continue;
        }

        // Create projection config excluding the placer and all sibling instances
        let mut projection = placer.projection.clone();
        projection.exclude_entities.push(instance.placer);
        if let Some(siblings) = instances_by_placer.get(&instance.placer) {
            projection.exclude_entities.extend(siblings.iter().copied());
        }

        // Pass source rotation for local-space direction transformation
        let source_rotation = if projection.local_space {
            Some(source_transform.to_scale_rotation_translation().1)
        } else {
            None
        };

        if let Some(result) = project_to_surface(
            &spatial_query,
            transform.translation,
            &projection,
            source_rotation,
        ) {
            let mut new_position = result.position;

            // Apply bounds offset if enabled - use instance's collider (instances are clones with their own colliders)
            if placer.use_bounds_offset {
                if let Ok(collider) = colliders.get(instance_entity) {
                    let aabb = collider.aabb(Vec3::ZERO, Quat::IDENTITY);
                    let half_extents: Vec3 = ((aabb.max - aabb.min) * 0.5).into();
                    let offset = half_extents.dot(result.normal.abs());
                    new_position += result.normal * offset;
                }
            }

            transform.translation = new_position;

            // Update rotation if AlignToSurface
            if matches!(placer.orientation, PlacementOrientation::AlignToSurface) {
                let up = result.normal.normalize_or_zero();
                if up.length_squared() > 0.001 {
                    let forward = if up.dot(Vec3::Z).abs() > 0.99 {
                        Vec3::X
                    } else {
                        Vec3::Z
                    };
                    let right = up.cross(forward).normalize();
                    let forward = right.cross(up).normalize();

                    transform.rotation = Quat::from_mat3(&Mat3::from_cols(right, up, forward));
                }
            }
        }
    }
}
