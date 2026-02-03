//! # bevy_grid_shader
//!
//! A Bevy plugin that provides a world-space grid shader material with PBR lighting support.
//!
//! The grid is rendered procedurally based on world coordinates, meaning:
//! - Grid lines align with world axes (not object UVs)
//! - Multiple objects share the same grid alignment
//! - Objects moving through space show the grid "sliding" across their surface
//!
//! ## Usage
//!
//! ```rust,no_run
//! use bevy::prelude::*;
//! use bevy_grid_shader::{GridMaterialPlugin, GridMaterial, GridAxes};
//!
//! fn main() {
//!     App::new()
//!         .add_plugins((DefaultPlugins, GridMaterialPlugin))
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//!
//! fn setup(
//!     mut commands: Commands,
//!     mut meshes: ResMut<Assets<Mesh>>,
//!     mut materials: ResMut<Assets<GridMaterial>>,
//! ) {
//!     commands.spawn((
//!         Mesh3d(meshes.add(Plane3d::default().mesh().size(10.0, 10.0))),
//!         MeshMaterial3d(materials.add(GridMaterial::default())),
//!     ));
//! }
//! ```

mod material;

pub use bevy::pbr::{ExtendedMaterial, StandardMaterial};
pub use material::{GridAxes, GridMaterial, GridMaterialPlugin};
