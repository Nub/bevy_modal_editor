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
//!     // Create named template entities (will be hidden automatically)
//!     commands.spawn((Name::new("Tree"), Mesh3d::default(), PlacementTemplate));
//!     commands.spawn((Name::new("Rock"), Mesh3d::default(), PlacementTemplate));
//!
//!     // Add ProceduralPlacer to any mesh - it samples its own surface
//!     // Templates are referenced by name for stable serialization.
//!     commands.spawn((
//!         Name::new("Terrain"),
//!         Mesh3d::default(), // terrain mesh
//!         ProceduralPlacer::new(vec![
//!             WeightedTemplate::new("Tree", 1.0),
//!             WeightedTemplate::new("Rock", 0.5),
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
    PlacementOrientation, ProceduralEntity, ProceduralPlacer, ProceduralTemplate, ResolvedPlacer,
    ResolvedTemplate, SamplingMode, SurfaceProjection, WeightedTemplate,
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
                (
                    hide_template_entities,
                    resolve_placer_templates,
                    update_placements,
                    cleanup_placements,
                )
                    .chain(),
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

/// System that resolves `WeightedTemplate.name` → `ResolvedPlacer` with entity references.
fn resolve_placer_templates(
    mut commands: Commands,
    placers: Query<
        (Entity, &ProceduralPlacer, Option<&ResolvedPlacer>),
        Or<(Changed<ProceduralPlacer>, Without<ResolvedPlacer>)>,
    >,
    named_entities: Query<(Entity, &Name)>,
) {
    for (entity, placer, _existing) in &placers {
        let resolved_templates: Vec<ResolvedTemplate> = placer
            .templates
            .iter()
            .filter_map(|wt| {
                let found = named_entities
                    .iter()
                    .find(|(_, name)| name.as_str() == wt.name);
                found.map(|(e, _)| ResolvedTemplate {
                    entity: e,
                    weight: wt.weight,
                })
            })
            .collect();

        commands
            .entity(entity)
            .insert(ResolvedPlacer { templates: resolved_templates });
    }
}

/// System to update placements when placer or source changes.
fn update_placements(world: &mut World) {
    // Collect placers that need updating.
    // Triggers on: ProceduralPlacer change, Mesh3d change, or GlobalTransform change.
    let mesh_placers: Vec<_> = world
        .query_filtered::<(
            Entity,
            &ProceduralPlacer,
            &ResolvedPlacer,
            &Mesh3d,
            &GlobalTransform,
            &Name,
        ), Or<(
            Changed<ProceduralPlacer>,
            Added<ProceduralPlacer>,
            Changed<Mesh3d>,
            Changed<GlobalTransform>,
        )>>()
        .iter(world)
        .filter(|(_, p, _, _, _, _)| p.enabled)
        .map(|(e, p, r, m, t, n)| (e, p.clone(), r.clone(), m.0.clone(), *t, n.as_str().to_string()))
        .collect();

    // Collect spline-based placers.
    #[cfg(feature = "spline")]
    let spline_placers: Vec<_> = world
        .query_filtered::<(
            Entity,
            &ProceduralPlacer,
            &ResolvedPlacer,
            &bevy_spline_3d::spline::Spline,
            &GlobalTransform,
            &Name,
        ), (
            Or<(
                Changed<ProceduralPlacer>,
                Added<ProceduralPlacer>,
                Changed<bevy_spline_3d::spline::Spline>,
                Changed<GlobalTransform>,
            )>,
            Without<Mesh3d>,
        )>()
        .iter(world)
        .filter(|(_, p, _, _, _, _)| p.enabled)
        .map(|(e, p, r, s, t, n)| (e, p.clone(), r.clone(), s.clone(), *t, n.as_str().to_string()))
        .collect();

    // Collect existing instances to remove (keyed by placer name)
    let instances_to_remove: Vec<_> = world
        .query::<(Entity, &ProceduralEntity)>()
        .iter(world)
        .map(|(e, i)| (e, i.placer.clone()))
        .collect();

    // First, sample all meshes while we have immutable access
    let mesh_samples: Vec<_> = {
        let meshes = world.resource::<Assets<Mesh>>();
        mesh_placers
            .iter()
            .map(|(placer_entity, placer, resolved, mesh_handle, global_transform, placer_name)| {
                let samples = meshes
                    .get(mesh_handle.id())
                    .map(|mesh| {
                        mesh.sample(
                            placer.count,
                            placer.mode.is_uniform(),
                            placer.mode.seed(),
                        )
                    })
                    .unwrap_or_default();
                (
                    *placer_entity,
                    placer.clone(),
                    resolved.clone(),
                    samples,
                    *global_transform,
                    placer_name.clone(),
                )
            })
            .collect()
    };

    // Process mesh-based placers
    for (placer_entity, placer, resolved, samples, global_transform, placer_name) in mesh_samples {
        // Remove existing instances for this placer
        for (instance_entity, instance_placer_name) in &instances_to_remove {
            if *instance_placer_name == placer_name {
                world.despawn(*instance_entity);
            }
        }

        if samples.is_empty() {
            continue;
        }

        spawn_instances_world(
            world,
            placer_entity,
            &placer_name,
            &placer,
            &resolved,
            &samples,
            &global_transform,
        );
    }

    // Process spline-based placers
    #[cfg(feature = "spline")]
    for (placer_entity, placer, resolved, spline, global_transform, placer_name) in &spline_placers
    {
        // Remove existing instances for this placer
        for (instance_entity, instance_placer_name) in &instances_to_remove {
            if instance_placer_name == placer_name {
                world.despawn(*instance_entity);
            }
        }

        let samples = spline_sampling::sample_spline(
            spline,
            placer.count,
            placer.mode.is_uniform(),
            placer.mode.seed(),
        );

        spawn_instances_world(
            world,
            *placer_entity,
            placer_name,
            placer,
            resolved,
            &samples,
            global_transform,
        );
    }
}

/// Spawn instances from samples using entity cloning.
fn spawn_instances_world(
    world: &mut World,
    _placer_entity: Entity,
    placer_name: &str,
    placer: &ProceduralPlacer,
    resolved: &ResolvedPlacer,
    samples: &[Sample],
    source_transform: &GlobalTransform,
) {
    if resolved.templates.is_empty() {
        return;
    }

    // Build weighted selection from resolved templates
    let total_weight: f32 = resolved.templates.iter().map(|t| t.weight).sum();
    if total_weight <= 0.0 {
        return;
    }

    let mut rng = fastrand::Rng::with_seed(placer.mode.seed().unwrap_or(0));

    // Collect spawn data first to avoid borrow issues
    let spawn_data: Vec<_> = samples
        .iter()
        .enumerate()
        .filter_map(|(index, sample)| {
            // Select template based on weight
            let template =
                select_weighted_resolved_template(&resolved.templates, total_weight, &mut rng);

            // Get template transform and name
            let template_transform = world.get::<Transform>(template.entity).copied()?;
            let template_name = world
                .get::<Name>(template.entity)
                .map(|n| n.as_str().to_string())
                .unwrap_or_default();

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

            Some((
                template.entity,
                template_name,
                offset_position,
                rotation,
                template_transform.scale,
                index,
            ))
        })
        .collect();

    // Now spawn instances
    for (template_entity, template_name, position, rotation, scale, index) in spawn_data {
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
        world
            .entity_mut(cloned)
            .insert(ProceduralEntity {
                placer: placer_name.to_string(),
                template: template_name,
                index,
            })
            .remove::<ProceduralTemplate>();

        // Make visible (templates are hidden)
        if let Some(mut visibility) = world.get_mut::<Visibility>(cloned) {
            *visibility = Visibility::Inherited;
        }
    }
}

/// Select a resolved template based on weights.
fn select_weighted_resolved_template<'a>(
    templates: &'a [ResolvedTemplate],
    total_weight: f32,
    rng: &mut fastrand::Rng,
) -> &'a ResolvedTemplate {
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
    placers_with_names: Query<&Name, With<ProceduralPlacer>>,
    instances: Query<(Entity, &ProceduralEntity)>,
) {
    for removed_placer in removed_placers.read() {
        // The entity is being removed, so we can't query its Name anymore.
        // Instead, despawn all instances whose placer entity no longer exists.
        // We check if the placer entity still has a ProceduralPlacer — if not, clean up.
        if placers_with_names.get(removed_placer).is_ok() {
            continue; // Still exists, skip
        }
        for (instance_entity, _instance) in &instances {
            // Since we can't know the name of the removed placer, we despawn all instances
            // referencing any placer that no longer exists. This is safe because
            // update_placements will recreate instances for still-existing placers.
            commands.entity(instance_entity).despawn();
        }
    }
}

/// System to project placed instances onto surfaces.
fn project_placed_instances(
    spatial_query: SpatialQuery,
    placers: Query<(Entity, &Name, &ProceduralPlacer, &GlobalTransform)>,
    mut instances: Query<(Entity, &ProceduralEntity, &mut Transform)>,
    colliders: Query<&Collider>,
) {
    use std::collections::HashMap;

    // Build name→entity lookup for placers
    let placer_by_name: HashMap<&str, (Entity, &ProceduralPlacer, &GlobalTransform)> = placers
        .iter()
        .map(|(e, n, p, t)| (n.as_str(), (e, p, t)))
        .collect();

    // First, collect all instance entities grouped by placer name
    let mut instances_by_placer: HashMap<String, Vec<Entity>> = HashMap::new();
    for (entity, instance, _) in instances.iter() {
        instances_by_placer
            .entry(instance.placer.clone())
            .or_default()
            .push(entity);
    }

    for (instance_entity, instance, mut transform) in &mut instances {
        let Some(&(placer_entity, placer, source_transform)) =
            placer_by_name.get(instance.placer.as_str())
        else {
            continue;
        };

        if !placer.projection.enabled {
            continue;
        }

        // Create projection config excluding the placer and all sibling instances
        let mut projection = placer.projection.clone();
        projection.exclude_entities.push(placer_entity);
        if let Some(siblings) = instances_by_placer.get(&instance.placer) {
            projection
                .exclude_entities
                .extend(siblings.iter().copied());
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

            // Apply bounds offset if enabled
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
