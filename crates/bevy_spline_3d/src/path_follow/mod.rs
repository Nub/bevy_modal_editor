//! Path following plugin for animating entities along splines.
//!
//! This plugin provides components and systems for moving entities along spline paths,
//! useful for camera rails, AI paths, moving platforms, and more.
//!
//! # Example
//!
//! ```rust,ignore
//! use bevy::prelude::*;
//! use bevy_spline_3d::prelude::*;
//!
//! fn setup(mut commands: Commands) {
//!     // Create a spline
//!     let spline_entity = commands.spawn(Spline::new(
//!         SplineType::CatmullRom,
//!         vec![
//!             Vec3::new(0.0, 0.0, 0.0),
//!             Vec3::new(5.0, 2.0, 0.0),
//!             Vec3::new(10.0, 0.0, 0.0),
//!         ],
//!     )).id();
//!
//!     // Spawn an entity that follows the spline
//!     commands.spawn((
//!         Transform::default(),
//!         SplineFollower {
//!             spline: spline_entity,
//!             speed: 2.0,
//!             ..default()
//!         },
//!     ));
//! }
//! ```

mod components;
mod systems;

pub use components::*;
pub use systems::update_spline_followers;

use bevy::prelude::*;

/// Plugin that enables entities to follow spline paths.
///
/// Add this plugin to your app, then add [`SplineFollower`] components to entities
/// you want to move along splines.
pub struct SplineFollowPlugin;

impl Plugin for SplineFollowPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<SplineFollower>()
            .register_type::<LoopMode>()
            .register_type::<FollowerState>()
            .add_message::<FollowerEvent>()
            .add_systems(Update, systems::update_spline_followers);
    }
}
