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
/// This plugin allows you to create copies of a template entity distributed
/// evenly along a spline curve.
///
/// # Usage
///
/// ```ignore
/// use bevy::prelude::*;
/// use bevy_spline_3d::prelude::*;
/// use bevy_spline_3d::distribution::*;
///
/// fn setup(mut commands: Commands, asset_server: Res<AssetServer>) {
///     // Create a spline
///     let spline = commands.spawn(Spline::new(
///         SplineType::CatmullRom,
///         vec![/* points */],
///     )).id();
///
///     // Create a template entity (this will be hidden)
///     let template = commands.spawn((
///         Mesh3d(asset_server.load("tree.glb")),
///         MeshMaterial3d(/* material */),
///         DistributionSource,
///     )).id();
///
///     // Create the distribution
///     commands.spawn(SplineDistribution {
///         spline,
///         source: template,
///         count: 10,
///         orientation: DistributionOrientation::AlignToTangent { up: Vec3::Y },
///         offset: Vec3::ZERO,
///     });
/// }
/// ```
///
/// # Orientation Modes
///
/// - `PositionOnly`: Only position is set, rotation remains at default
/// - `AlignToTangent`: Forward (negative Z) aligns to spline tangent with specified up vector
///
/// # Spacing Modes
///
/// - `Uniform`: Even arc-length spacing (default, recommended)
/// - `Parametric`: Based on spline t parameter (faster but uneven)
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
