//! Blockout shapes for rapid level prototyping
//!
//! Provides parametric shapes like stairs, ramps, arches, and L-shapes
//! with configurable parameters and automatic mesh/collider regeneration.

mod mesh_gen;

use avian3d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub use mesh_gen::*;

use super::SceneEntity;

/// Available blockout shapes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockoutShape {
    #[default]
    Stairs,
    Ramp,
    Arch,
    LShape,
}

impl BlockoutShape {
    pub fn display_name(&self) -> &'static str {
        match self {
            BlockoutShape::Stairs => "Stairs",
            BlockoutShape::Ramp => "Ramp",
            BlockoutShape::Arch => "Arch",
            BlockoutShape::LShape => "L-Shape",
        }
    }
}

/// Marker component for parametric stairs
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct StairsMarker {
    /// Number of steps
    pub step_count: u32,
    /// Total height of stairs
    pub height: f32,
    /// Total depth of stairs (horizontal distance)
    pub depth: f32,
    /// Width of stairs
    pub width: f32,
}

impl Default for StairsMarker {
    fn default() -> Self {
        Self {
            step_count: 8,
            height: 2.0,
            depth: 4.0,
            width: 2.0,
        }
    }
}

/// Marker component for ramp/wedge shapes
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct RampMarker {
    /// Height at the top of the ramp
    pub height: f32,
    /// Length of the ramp (horizontal distance)
    pub length: f32,
    /// Width of the ramp
    pub width: f32,
}

impl Default for RampMarker {
    fn default() -> Self {
        Self {
            height: 2.0,
            length: 4.0,
            width: 2.0,
        }
    }
}

/// Marker component for arch/doorway shapes
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct ArchMarker {
    /// Width of the opening
    pub opening_width: f32,
    /// Height of the opening (from ground to arch start)
    pub opening_height: f32,
    /// Wall thickness (depth)
    pub thickness: f32,
    /// Overall wall width
    pub wall_width: f32,
    /// Overall wall height
    pub wall_height: f32,
    /// Number of segments for the arch curve
    pub arch_segments: u32,
}

impl Default for ArchMarker {
    fn default() -> Self {
        Self {
            opening_width: 2.0,
            opening_height: 2.5,
            thickness: 0.5,
            wall_width: 4.0,
            wall_height: 4.0,
            arch_segments: 8,
        }
    }
}

/// Marker component for L-shape corner pieces
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct LShapeMarker {
    /// Length of one arm (along +X)
    pub arm1_length: f32,
    /// Length of the other arm (along +Z)
    pub arm2_length: f32,
    /// Width/thickness of the arms
    pub arm_width: f32,
    /// Height of the shape
    pub height: f32,
}

impl Default for LShapeMarker {
    fn default() -> Self {
        Self {
            arm1_length: 3.0,
            arm2_length: 3.0,
            arm_width: 1.0,
            height: 3.0,
        }
    }
}

/// Default colors for blockout shapes
pub mod blockout_colors {
    use bevy::prelude::*;

    pub const STAIRS: Color = Color::srgb(0.65, 0.65, 0.7);
    pub const RAMP: Color = Color::srgb(0.7, 0.65, 0.65);
    pub const ARCH: Color = Color::srgb(0.65, 0.7, 0.65);
    pub const LSHAPE: Color = Color::srgb(0.7, 0.7, 0.65);
}

pub struct BlockoutPlugin;

impl Plugin for BlockoutPlugin {
    fn build(&self, app: &mut App) {
        app.register_type::<StairsMarker>()
            .register_type::<RampMarker>()
            .register_type::<ArchMarker>()
            .register_type::<LShapeMarker>()
            .add_systems(
                Update,
                (
                    regenerate_stairs_mesh,
                    regenerate_ramp_mesh,
                    regenerate_arch_mesh,
                    regenerate_lshape_mesh,
                ),
            );
    }
}

/// Spawn a stairs entity
pub fn spawn_stairs(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    rotation: Quat,
    name: &str,
) -> Entity {
    let marker = StairsMarker::default();
    let mesh = generate_stairs_mesh(&marker);
    let collider = generate_stairs_collider(&marker);

    commands
        .spawn((
            SceneEntity,
            Name::new(name.to_string()),
            marker,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: blockout_colors::STAIRS,
                ..default()
            })),
            Transform::from_translation(position).with_rotation(rotation),
            RigidBody::Static,
            collider,
        ))
        .id()
}

/// Spawn a ramp entity
pub fn spawn_ramp(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    rotation: Quat,
    name: &str,
) -> Entity {
    let marker = RampMarker::default();
    let mesh = generate_ramp_mesh(&marker);
    let collider = generate_ramp_collider(&marker);

    commands
        .spawn((
            SceneEntity,
            Name::new(name.to_string()),
            marker,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: blockout_colors::RAMP,
                ..default()
            })),
            Transform::from_translation(position).with_rotation(rotation),
            RigidBody::Static,
            collider,
        ))
        .id()
}

/// Spawn an arch entity
pub fn spawn_arch(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    rotation: Quat,
    name: &str,
) -> Entity {
    let marker = ArchMarker::default();
    let mesh = generate_arch_mesh(&marker);
    let collider = generate_arch_collider(&marker);

    commands
        .spawn((
            SceneEntity,
            Name::new(name.to_string()),
            marker,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: blockout_colors::ARCH,
                ..default()
            })),
            Transform::from_translation(position).with_rotation(rotation),
            RigidBody::Static,
            collider,
        ))
        .id()
}

/// Spawn an L-shape entity
pub fn spawn_lshape(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    rotation: Quat,
    name: &str,
) -> Entity {
    let marker = LShapeMarker::default();
    let mesh = generate_lshape_mesh(&marker);
    let collider = generate_lshape_collider(&marker);

    commands
        .spawn((
            SceneEntity,
            Name::new(name.to_string()),
            marker,
            Mesh3d(meshes.add(mesh)),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: blockout_colors::LSHAPE,
                ..default()
            })),
            Transform::from_translation(position).with_rotation(rotation),
            RigidBody::Static,
            collider,
        ))
        .id()
}

/// Regenerate stairs mesh when parameters change
fn regenerate_stairs_mesh(
    mut query: Query<(Entity, &StairsMarker, &mut Mesh3d), Changed<StairsMarker>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    for (entity, marker, mut mesh_handle) in query.iter_mut() {
        let new_mesh = generate_stairs_mesh(marker);
        mesh_handle.0 = meshes.add(new_mesh);

        let collider = generate_stairs_collider(marker);
        commands.entity(entity).insert(collider);
    }
}

/// Regenerate ramp mesh when parameters change
fn regenerate_ramp_mesh(
    mut query: Query<(Entity, &RampMarker, &mut Mesh3d), Changed<RampMarker>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    for (entity, marker, mut mesh_handle) in query.iter_mut() {
        let new_mesh = generate_ramp_mesh(marker);
        mesh_handle.0 = meshes.add(new_mesh);

        let collider = generate_ramp_collider(marker);
        commands.entity(entity).insert(collider);
    }
}

/// Regenerate arch mesh when parameters change
fn regenerate_arch_mesh(
    mut query: Query<(Entity, &ArchMarker, &mut Mesh3d), Changed<ArchMarker>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    for (entity, marker, mut mesh_handle) in query.iter_mut() {
        let new_mesh = generate_arch_mesh(marker);
        mesh_handle.0 = meshes.add(new_mesh);

        let collider = generate_arch_collider(marker);
        commands.entity(entity).insert(collider);
    }
}

/// Regenerate L-shape mesh when parameters change
fn regenerate_lshape_mesh(
    mut query: Query<(Entity, &LShapeMarker, &mut Mesh3d), Changed<LShapeMarker>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut commands: Commands,
) {
    for (entity, marker, mut mesh_handle) in query.iter_mut() {
        let new_mesh = generate_lshape_mesh(marker);
        mesh_handle.0 = meshes.add(new_mesh);

        let collider = generate_lshape_collider(marker);
        commands.entity(entity).insert(collider);
    }
}
