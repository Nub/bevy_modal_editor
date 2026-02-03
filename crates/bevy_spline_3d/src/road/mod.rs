mod intersection;
mod mesh_gen;
mod projection;

pub use intersection::*;
pub use mesh_gen::*;
pub use projection::NeedsProjection;

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::spline::SplinePlugin;

/// Plugin for generating road meshes that follow splines.
///
/// This plugin allows you to extrude a cross-section mesh along a spline,
/// creating seamless road geometry that follows curves.
///
/// # Mesh Requirements
///
/// The source mesh must be a **cross-section segment** with specific orientation:
///
/// ```text
///        Y (up)
///        │
///        │    ┌─────────────┐
///        │    │  Road Top   │
///        │    │             │
///        └────┼─────────────┼────► X (width)
///             │             │
///             └─────────────┘
///                   │
///                   ▼ Z (forward/extrusion direction)
/// ```
///
/// ## Coordinate System
/// - **X axis**: Road width (left edge at -X, right edge at +X)
/// - **Y axis**: Road height/thickness (surface at Y=0 or higher)
/// - **Z axis**: Forward direction (the mesh will be extruded along this axis)
///
/// ## Mesh Structure
/// The mesh should represent a **single segment** that can tile seamlessly:
/// - Front edge at `Z = 0`
/// - Back edge at `Z = segment_length` (any positive value)
/// - Vertices at Z=0 will be stitched with the previous segment
/// - Vertices at Z=max will be stitched with the next segment
///
/// ## Vertex Requirements
/// - Vertices must form a consistent cross-section at both Z=0 and Z=max
/// - The number and order of vertices at both ends must match for seamless stitching
/// - Include proper UV coordinates (U = across width, V = along length)
///
/// # Example Mesh Creation
///
/// ```ignore
/// // Simple flat road (4 vertices per end, 2 triangles per segment)
/// let road_width = 4.0;
/// let segment_length = 2.0;
///
/// let vertices = vec![
///     // Front edge (Z = 0)
///     [-road_width/2., 0., 0.],
///     [ road_width/2., 0., 0.],
///     // Back edge (Z = segment_length)
///     [-road_width/2., 0., segment_length],
///     [ road_width/2., 0., segment_length],
/// ];
/// ```
///
/// # Usage
///
/// ```ignore
/// use bevy_spline_3d::prelude::*;
/// use bevy_spline_3d::road::*;
///
/// fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>) {
///     let spline = commands.spawn(Spline::new(/* ... */)).id();
///
///     // Create or load a road segment mesh
///     let road_segment = create_road_segment_mesh(4.0, 2.0, 0.1);
///     let segment_handle = meshes.add(road_segment);
///
///     commands.spawn(SplineRoad {
///         spline,
///         segment_mesh: segment_handle,
///         segments_per_curve: 32,
///         ..default()
///     });
/// }
/// ```
pub struct SplineRoadPlugin;

impl Plugin for SplineRoadPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<SplinePlugin>() {
            app.add_plugins(SplinePlugin);
        }

        app.register_type::<SplineRoad>()
            .register_type::<RoadIntersection>()
            .register_type::<RoadEnd>()
            .register_type::<RoadConnection>()
            .add_systems(
                Update,
                (
                    mesh_gen::update_road_meshes,
                    intersection::update_intersection_meshes,
                    intersection::cleanup_intersection_meshes,
                ),
            );

        // Run projection in PostUpdate after transform propagation
        // to ensure GlobalTransform is up to date.
        // Only runs when avian3d physics is available.
        app.add_systems(
            PostUpdate,
            projection::project_road_meshes
                .after(TransformSystems::Propagate)
                .run_if(projection::physics_available),
        );
    }
}

/// Component that defines a road mesh generated along a spline.
#[derive(Component, Debug, Clone, Reflect)]
#[reflect(Component)]
pub struct SplineRoad {
    /// The spline entity to follow.
    pub spline: Entity,
    /// Handle to the source segment mesh (cross-section).
    #[reflect(ignore)]
    pub segment_mesh: Handle<Mesh>,
    /// Number of segments to generate along the spline.
    /// Higher values = smoother curves but more geometry.
    pub segments_per_curve: usize,
    /// Whether to automatically update when the spline changes.
    pub auto_update: bool,
    /// UV tiling factor along the road length.
    /// Higher values = more texture repeats.
    pub uv_tile_length: f32,
}

impl Default for SplineRoad {
    fn default() -> Self {
        Self {
            spline: Entity::PLACEHOLDER,
            segment_mesh: Handle::default(),
            segments_per_curve: 32,
            auto_update: true,
            uv_tile_length: 1.0,
        }
    }
}

impl SplineRoad {
    /// Create a new road configuration.
    pub fn new(spline: Entity, segment_mesh: Handle<Mesh>) -> Self {
        Self {
            spline,
            segment_mesh,
            ..default()
        }
    }

    /// Set the number of segments per curve.
    pub fn with_segments(mut self, segments: usize) -> Self {
        self.segments_per_curve = segments;
        self
    }

    /// Set the UV tiling factor.
    pub fn with_uv_tile(mut self, tile: f32) -> Self {
        self.uv_tile_length = tile;
        self
    }
}

/// Marker component for the generated road mesh entity.
#[derive(Component, Debug, Clone, Copy)]
pub struct GeneratedRoadMesh {
    /// The SplineRoad entity this mesh belongs to.
    pub road: Entity,
}
