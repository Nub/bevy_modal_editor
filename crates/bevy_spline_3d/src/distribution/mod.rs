//! Spline distribution module.
//!
//! **Deprecated:** Use `bevy_procedural` crate instead for more flexible
//! procedural placement along splines and volumes.

#![allow(deprecated)] // Allow deprecated within this module during migration

mod components;
mod projection;
mod systems;

pub use components::*;
pub use projection::NeedsInstanceProjection;

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::spline::SplinePlugin;

/// Plugin for distributing entities along splines.
///
/// **Deprecated:** Use `bevy_procedural::ProceduralPlugin` instead, which provides
/// a more flexible and composable API for procedural placement along splines,
/// as well as support for volume-based placement (boxes, spheres, cylinders).
///
/// # Migration
/// ```ignore
/// // Old (SplineDistributionPlugin)
/// app.add_plugins(SplineDistributionPlugin);
/// commands.spawn(SplineDistribution::new(spline, template, 10)
///     .with_orientation(DistributionOrientation::align_to_tangent())
///     .uniform());
///
/// // New (bevy_procedural)
/// app.add_plugins(ProceduralPlugin);
/// commands.spawn((
///     Sampler::uniform(10),
///     Placer::new(spline, template)
///         .with_orientation(PlacementOrientation::AlignToTangent { up: Vec3::Y }),
/// ));
/// ```
#[deprecated(since = "0.2.0", note = "Use bevy_procedural::ProceduralPlugin instead")]
#[allow(deprecated)]
pub struct SplineDistributionPlugin;

impl Plugin for SplineDistributionPlugin {
    fn build(&self, app: &mut App) {
        // Ensure SplinePlugin is added
        if !app.is_plugin_added::<SplinePlugin>() {
            app.add_plugins(SplinePlugin);
        }

        app.register_type::<SplineDistribution>()
            .register_type::<DistributionOrientation>()
            .register_type::<DistributionSpacing>()
            .register_type::<DistributionSource>()
            .register_type::<DistributedInstance>()
            .add_systems(
                Update,
                (
                    systems::hide_source_entities,
                    systems::update_distributions,
                    systems::cleanup_distributions,
                )
                    .chain(),
            );

        // Run projection in PostUpdate after transform propagation.
        // Only runs when avian3d physics is available.
        app.add_systems(
            PostUpdate,
            projection::project_distributed_instances
                .after(TransformSystems::Propagate)
                .run_if(projection::physics_available),
        );
    }
}
