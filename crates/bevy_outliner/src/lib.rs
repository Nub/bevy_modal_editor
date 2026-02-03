//! # Bevy Outliner
//!
//! A silhouette-based object outlining plugin for Bevy 0.18.
//!
//! Add outlines to any mesh by simply adding the [`MeshOutline`] component.
//!
//! ## Quick Start
//!
//! ```no_run
//! use bevy::prelude::*;
//! use bevy_outliner::prelude::*;
//!
//! fn main() {
//!     App::new()
//!         .add_plugins((DefaultPlugins, OutlinePlugin))
//!         .add_systems(Startup, setup)
//!         .run();
//! }
//!
//! fn setup(
//!     mut commands: Commands,
//!     mut meshes: ResMut<Assets<Mesh>>,
//!     mut materials: ResMut<Assets<StandardMaterial>>,
//! ) {
//!     // Spawn a cube with an orange outline
//!     commands.spawn((
//!         Mesh3d(meshes.add(Cuboid::default())),
//!         MeshMaterial3d(materials.add(Color::srgb(0.8, 0.2, 0.2))),
//!         MeshOutline::default(),
//!     ));
//!
//!     // Main camera with outline support
//!     commands.spawn((
//!         Camera3d::default(),
//!         Transform::from_xyz(0.0, 2.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
//!         OutlineSettings::default(),
//!     ));
//! }
//! ```

mod components;
mod jfa_material;
mod silhouette_material;

pub mod prelude {
    pub use crate::components::{MeshOutline, OutlineSettings};
    pub use crate::OutlinePlugin;
}

pub use components::*;

use bevy::{asset::embedded_asset, prelude::*};

use jfa_material::{
    resize_silhouette_textures, setup_outline_camera, sync_outline_meshes, sync_silhouette_cameras,
    OutlineRenderPlugin,
};
use silhouette_material::SilhouetteMaterial;

/// Plugin that enables silhouette-based object outlining.
pub struct OutlinePlugin;

impl Plugin for OutlinePlugin {
    fn build(&self, app: &mut App) {
        // Embed shaders
        embedded_asset!(app, "shaders/jfa_init_compute.wgsl");
        embedded_asset!(app, "shaders/jfa_step_compute.wgsl");
        embedded_asset!(app, "shaders/jfa_composite.wgsl");
        embedded_asset!(app, "shaders/silhouette.wgsl");

        app.add_plugins((
            OutlineRenderPlugin,
            MaterialPlugin::<SilhouetteMaterial>::default(),
        ))
        .add_systems(
            PostUpdate,
            (
                setup_outline_camera,
                sync_outline_meshes,
                sync_silhouette_cameras,
                resize_silhouette_textures,
            )
                .chain(),
        );
    }
}
