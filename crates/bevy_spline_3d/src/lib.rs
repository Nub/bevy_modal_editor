//! # bevy_spline_3d
//!
//! A Bevy plugin for 3D spline editing with interactive gizmos.
//!
//! ## Features
//!
//! - Multiple spline types: Cubic Bézier, Catmull-Rom, B-Spline
//! - Interactive control point editing with gizmos
//! - Serializable with Bevy's scene system (RON format)
//! - Orbit and fly camera controls
//! - Hotkeys for common operations
//!
//! ## Quick Start
//!
//! ```ignore
//! use bevy::prelude::*;
//! use bevy_spline_3d::prelude::*;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins(DefaultPlugins)
//!         .add_plugins(SplinePlugin)
//!         .add_plugins(SplineEditorPlugin)  // Optional: adds interactive editing
//!         .add_plugins(CameraPlugin)         // Optional: adds camera controls
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//!
//! fn setup(mut commands: Commands) {
//!     // Spawn a camera with orbit controls
//!     commands.spawn((
//!         Camera3d::default(),
//!         Transform::from_xyz(0.0, 5.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y),
//!         OrbitCamera::default(),
//!         FlyCamera::default(),
//!     ));
//!
//!     // Spawn a spline
//!     commands.spawn(Spline::new(
//!         SplineType::CatmullRom,
//!         vec![
//!             Vec3::new(-3.0, 0.0, 0.0),
//!             Vec3::new(-1.0, 2.0, 0.0),
//!             Vec3::new(1.0, -1.0, 0.0),
//!             Vec3::new(3.0, 1.0, 0.0),
//!         ],
//!     ));
//! }
//! ```
//!
//! ## Plugins
//!
//! - [`SplinePlugin`]: Core spline functionality and type registration (required)
//! - [`SplineEditorPlugin`]: Interactive editing with gizmos and hotkeys (optional)
//! - [`SplineDistributionPlugin`]: Distribute entities along splines (optional)
//! - [`SplineRoadPlugin`]: Generate road meshes along splines (optional)
//! - [`SplineFollowPlugin`]: Animate entities following spline paths (optional)
//! - [`CameraPlugin`]: Orbit and fly camera controls (optional)
//!
//! ## Disabling the Editor
//!
//! The editor can be toggled at runtime:
//!
//! ```ignore
//! fn toggle_editor(mut settings: ResMut<EditorSettings>) {
//!     settings.enabled = false;     // Disable input handling
//!     settings.show_gizmos = false; // Hide visual gizmos
//! }
//! ```

pub mod camera;
pub mod distribution;
pub mod geometry;
pub mod path_follow;
pub mod road;
pub mod spline;
pub mod surface;

#[cfg(feature = "editor")]
pub mod editor;

pub use camera::CameraPlugin;
pub use distribution::SplineDistributionPlugin;
pub use path_follow::SplineFollowPlugin;
pub use road::SplineRoadPlugin;
pub use spline::SplinePlugin;
pub use surface::SplineMeshProjectionPlugin;

#[cfg(feature = "editor")]
pub use editor::SplineEditorPlugin;

/// Convenient re-exports of commonly used types.
pub mod prelude {
    pub use crate::camera::{CameraMode, CameraPlugin, FlyCamera, OrbitCamera};
    pub use crate::distribution::{
        DistributedInstance, DistributionOrientation, DistributionSource, DistributionSpacing,
        SplineDistribution, SplineDistributionPlugin,
    };
    pub use crate::path_follow::{
        FollowerEvent, FollowerEventKind, FollowerState, LoopMode, SplineFollowPlugin,
        SplineFollower,
    };
    pub use crate::road::{
        create_road_segment_mesh, GeneratedIntersectionMesh, GeneratedRoadMesh,
        RoadConnection, RoadEnd, RoadIntersection, SplineRoad, SplineRoadPlugin,
    };
    pub use crate::spline::{
        CachedSplineCurve, ControlPointMarker, ProjectedSplineCache, SelectedControlPoint,
        SelectedSpline, Spline, SplineEvaluator, SplinePlugin, SplineType,
        get_effective_control_points, get_effective_curve_points,
    };

    #[cfg(feature = "editor")]
    pub use crate::editor::{
        EditorSettings, GizmoColors, GizmoSizes, GizmoVisuals, SelectionState, SplineEditorPlugin,
    };

    pub use crate::surface::{
        ProjectionHit, ProjectionLayer, RawProjectionHit, SplineMeshProjection,
        SplineMeshProjectionPlugin, cast_projection_ray, create_projection_filter,
    };

    pub use crate::geometry::CoordinateFrame;
}
