use avian3d::prelude::*;
use bevy::light::FogVolume;
use bevy::prelude::*;
use bevy_spline_3d::prelude::{Spline, SplineType};
use serde::{Deserialize, Serialize};

use super::blockout::{spawn_arch, spawn_lshape, spawn_ramp, spawn_stairs};
use super::SceneEntity;
use crate::constants::{light_colors, physics, primitive_colors};
use crate::selection::Selected;

/// Marker component for group entities (containers for nesting)
#[derive(Component, Serialize, Deserialize, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct GroupMarker;

/// Marker component for locked entities (prevents editing)
#[derive(Component, Serialize, Deserialize, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct Locked;

/// Marker component for point lights
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct SceneLightMarker {
    pub color: Color,
    pub intensity: f32,
    pub range: f32,
    pub shadows_enabled: bool,
}

impl Default for SceneLightMarker {
    fn default() -> Self {
        Self {
            color: light_colors::POINT_DEFAULT,
            intensity: light_colors::POINT_DEFAULT_INTENSITY,
            range: light_colors::POINT_DEFAULT_RANGE,
            shadows_enabled: true,
        }
    }
}

/// Marker component for directional lights (sun)
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct DirectionalLightMarker {
    pub color: Color,
    pub illuminance: f32,
    pub shadows_enabled: bool,
}

impl Default for DirectionalLightMarker {
    fn default() -> Self {
        Self {
            color: light_colors::DIRECTIONAL_DEFAULT,
            illuminance: light_colors::DIRECTIONAL_DEFAULT_ILLUMINANCE,
            shadows_enabled: true,
        }
    }
}

/// Marker component for spline entities
#[derive(Component, Serialize, Deserialize, Clone, Default, Reflect)]
#[reflect(Component)]
pub struct SplineMarker;

/// Marker component for fog volume entities (stores serializable fog settings)
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct FogVolumeMarker {
    pub fog_color: Color,
    pub density_factor: f32,
    pub absorption: f32,
    pub scattering: f32,
    pub scattering_asymmetry: f32,
    pub light_tint: Color,
    pub light_intensity: f32,
}

impl Default for FogVolumeMarker {
    fn default() -> Self {
        Self {
            fog_color: Color::WHITE,
            density_factor: 0.1,
            absorption: 0.3,
            scattering: 0.3,
            scattering_asymmetry: 0.8,
            light_tint: Color::WHITE,
            light_intensity: 1.0,
        }
    }
}

/// Marker indicating the material type for an entity
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
#[reflect(Component)]
pub enum MaterialType {
    #[default]
    Standard,
    Grid,
}

/// Available primitive shapes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Reflect, Default)]
pub enum PrimitiveShape {
    #[default]
    Cube,
    Sphere,
    Cylinder,
    Capsule,
    Plane,
}

impl PrimitiveShape {
    pub fn display_name(&self) -> &'static str {
        match self {
            PrimitiveShape::Cube => "Cube",
            PrimitiveShape::Sphere => "Sphere",
            PrimitiveShape::Cylinder => "Cylinder",
            PrimitiveShape::Capsule => "Capsule",
            PrimitiveShape::Plane => "Plane",
        }
    }

    /// Create the mesh for this primitive shape
    pub fn create_mesh(&self) -> Mesh {
        match self {
            PrimitiveShape::Cube => Mesh::from(Cuboid::new(1.0, 1.0, 1.0)),
            PrimitiveShape::Sphere => Mesh::from(Sphere::new(0.5)),
            PrimitiveShape::Cylinder => Mesh::from(Cylinder::new(0.5, 1.0)),
            PrimitiveShape::Capsule => Mesh::from(Capsule3d::new(0.25, 0.5)),
            PrimitiveShape::Plane => Plane3d::default().mesh().size(2.0, 2.0).build(),
        }
    }

    /// Get the default color for this primitive shape
    pub fn default_color(&self) -> Color {
        primitive_colors::for_shape(*self)
    }

    /// Create a standard material for this primitive shape
    pub fn create_material(&self) -> StandardMaterial {
        StandardMaterial {
            base_color: self.default_color(),
            ..default()
        }
    }

    /// Create the collider for this primitive shape
    pub fn create_collider(&self) -> Collider {
        match self {
            PrimitiveShape::Cube => Collider::cuboid(1.0, 1.0, 1.0),
            PrimitiveShape::Sphere => Collider::sphere(0.5),
            PrimitiveShape::Cylinder => Collider::cylinder(0.5, 0.5),
            PrimitiveShape::Capsule => Collider::capsule(0.25, 0.5),
            PrimitiveShape::Plane => Collider::cuboid(2.0, 0.01, 2.0),
        }
    }
}

/// The kind of entity to spawn
#[derive(Debug, Clone)]
pub enum SpawnEntityKind {
    /// A primitive shape (cube, sphere, etc.)
    Primitive(PrimitiveShape),
    /// An empty group for organizing entities
    Group,
    /// A point light
    PointLight,
    /// A directional light (sun)
    DirectionalLight,
    /// A spline curve
    Spline(SplineType),
    /// A volumetric fog volume
    FogVolume,
    /// Parametric stairs
    Stairs,
    /// Parametric ramp/wedge
    Ramp,
    /// Parametric arch/doorway
    Arch,
    /// Parametric L-shape corner
    LShape,
}

impl SpawnEntityKind {
    /// Get the display name for this entity kind
    pub fn display_name(&self) -> &'static str {
        match self {
            SpawnEntityKind::Primitive(shape) => shape.display_name(),
            SpawnEntityKind::Group => "Group",
            SpawnEntityKind::PointLight => "Point Light",
            SpawnEntityKind::DirectionalLight => "Sun",
            SpawnEntityKind::Spline(spline_type) => match spline_type {
                SplineType::CubicBezier => "Bezier Spline",
                SplineType::CatmullRom => "Catmull-Rom Spline",
                SplineType::BSpline => "B-Spline",
            },
            SpawnEntityKind::FogVolume => "Fog Volume",
            SpawnEntityKind::Stairs => "Stairs",
            SpawnEntityKind::Ramp => "Ramp",
            SpawnEntityKind::Arch => "Arch",
            SpawnEntityKind::LShape => "L-Shape",
        }
    }
}

/// Unified event to spawn any entity type
#[derive(Message)]
pub struct SpawnEntityEvent {
    pub kind: SpawnEntityKind,
    pub position: Vec3,
    pub rotation: Quat,
}

/// Event to parent selected entity to a target group
#[derive(Message)]
pub struct ParentToGroupEvent {
    pub child: Entity,
    pub parent: Entity,
}

/// Event to unparent an entity (move to root)
#[derive(Message)]
pub struct UnparentEvent {
    pub entity: Entity,
}

/// Event to unparent all selected entities (move to root)
#[derive(Message)]
pub struct UnparentSelectedEvent;

/// Event to group multiple selected entities into a new group
#[derive(Message)]
pub struct GroupSelectedEvent;

/// Component to track what primitive shape an entity is
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct PrimitiveMarker {
    pub shape: PrimitiveShape,
}

pub struct PrimitivesPlugin;

impl Plugin for PrimitivesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SpawnEntityEvent>()
            .add_message::<ParentToGroupEvent>()
            .add_message::<UnparentEvent>()
            .add_message::<UnparentSelectedEvent>()
            .add_message::<GroupSelectedEvent>()
            .add_systems(
                Update,
                (
                    handle_spawn_entity,
                    handle_parent_to_group,
                    handle_unparent,
                    handle_unparent_selected,
                    handle_group_selected,
                ),
            );
    }
}

/// Unified handler for spawning any entity type
fn handle_spawn_entity(
    mut events: MessageReader<SpawnEntityEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing_entities: Query<&Name, With<SceneEntity>>,
    selected_entities: Query<Entity, With<Selected>>,
) {
    for event in events.read() {
        // Deselect all currently selected entities
        for entity in selected_entities.iter() {
            commands.entity(entity).remove::<Selected>();
        }

        let name = generate_unique_name(event.kind.display_name(), &existing_entities);

        let new_entity = match &event.kind {
            SpawnEntityKind::Primitive(shape) => spawn_primitive(
                &mut commands,
                &mut meshes,
                &mut materials,
                *shape,
                event.position,
                event.rotation,
                &name,
            ),
            SpawnEntityKind::Group => spawn_group(&mut commands, event.position, event.rotation, &name),
            SpawnEntityKind::PointLight => spawn_point_light(&mut commands, event.position, event.rotation, &name),
            SpawnEntityKind::DirectionalLight => spawn_directional_light(&mut commands, event.position, event.rotation, &name),
            SpawnEntityKind::Spline(spline_type) => spawn_spline(&mut commands, *spline_type, event.position, event.rotation, &name),
            SpawnEntityKind::FogVolume => spawn_fog_volume(&mut commands, event.position, event.rotation, &name),
            SpawnEntityKind::Stairs => spawn_stairs(&mut commands, &mut meshes, &mut materials, event.position, event.rotation, &name),
            SpawnEntityKind::Ramp => spawn_ramp(&mut commands, &mut meshes, &mut materials, event.position, event.rotation, &name),
            SpawnEntityKind::Arch => spawn_arch(&mut commands, &mut meshes, &mut materials, event.position, event.rotation, &name),
            SpawnEntityKind::LShape => spawn_lshape(&mut commands, &mut meshes, &mut materials, event.position, event.rotation, &name),
        };

        // Select the newly spawned entity
        commands.entity(new_entity).insert(Selected);
    }
}

/// Generate a unique name by appending a counter
pub fn generate_unique_name(base: &str, existing: &Query<&Name, With<SceneEntity>>) -> String {
    let mut counter = 1;
    loop {
        let name = format!("{} {}", base, counter);
        let exists = existing.iter().any(|n| n.as_str() == name);
        if !exists {
            return name;
        }
        counter += 1;
    }
}

/// Spawn a primitive shape entity with all required components
pub fn spawn_primitive(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    shape: PrimitiveShape,
    position: Vec3,
    rotation: Quat,
    name: &str,
) -> Entity {
    commands
        .spawn((
            SceneEntity,
            Name::new(name.to_string()),
            PrimitiveMarker { shape },
            MaterialType::Standard,
            Mesh3d(meshes.add(shape.create_mesh())),
            MeshMaterial3d(materials.add(shape.create_material())),
            Transform::from_translation(position).with_rotation(rotation),
            RigidBody::Static,
            shape.create_collider(),
        ))
        .id()
}

/// Spawn an empty group entity
pub fn spawn_group(commands: &mut Commands, position: Vec3, rotation: Quat, name: &str) -> Entity {
    commands
        .spawn((
            SceneEntity,
            GroupMarker,
            Name::new(name.to_string()),
            Transform::from_translation(position).with_rotation(rotation),
            Visibility::default(),
        ))
        .id()
}

/// Spawn a point light entity
pub fn spawn_point_light(commands: &mut Commands, position: Vec3, rotation: Quat, name: &str) -> Entity {
    let light_marker = SceneLightMarker::default();
    commands
        .spawn((
            SceneEntity,
            Name::new(name.to_string()),
            light_marker.clone(),
            PointLight {
                color: light_marker.color,
                intensity: light_marker.intensity,
                range: light_marker.range,
                shadows_enabled: light_marker.shadows_enabled,
                ..default()
            },
            Transform::from_translation(position).with_rotation(rotation),
            Visibility::default(),
            Collider::sphere(physics::LIGHT_COLLIDER_RADIUS),
        ))
        .id()
}

/// Spawn a directional light (sun) entity
pub fn spawn_directional_light(commands: &mut Commands, position: Vec3, rotation: Quat, name: &str) -> Entity {
    let light_marker = DirectionalLightMarker::default();
    commands
        .spawn((
            SceneEntity,
            Name::new(name.to_string()),
            light_marker.clone(),
            DirectionalLight {
                color: light_marker.color,
                illuminance: light_marker.illuminance,
                shadows_enabled: light_marker.shadows_enabled,
                ..default()
            },
            Transform::from_translation(position).with_rotation(rotation),
            Visibility::default(),
            Collider::sphere(physics::LIGHT_COLLIDER_RADIUS),
        ))
        .id()
}

/// Spawn a spline entity with default control points
pub fn spawn_spline(commands: &mut Commands, spline_type: SplineType, position: Vec3, rotation: Quat, name: &str) -> Entity {
    // Create default control points relative to the spline's position
    // Points extend along the local X axis
    let default_points = vec![
        Vec3::new(-2.0, 0.0, 0.0),
        Vec3::new(-0.5, 1.0, 0.0),
        Vec3::new(0.5, -1.0, 0.0),
        Vec3::new(2.0, 0.0, 0.0),
    ];

    let spline = Spline::new(spline_type, default_points);

    // Splines don't use physics colliders for selection - they use proximity-based
    // picking in the selection system to avoid blocking clicks on objects inside/behind them
    commands
        .spawn((
            SceneEntity,
            SplineMarker,
            Name::new(name.to_string()),
            spline,
            Transform::from_translation(position).with_rotation(rotation),
            Visibility::default(),
        ))
        .id()
}

/// Spawn a fog volume entity
pub fn spawn_fog_volume(commands: &mut Commands, position: Vec3, rotation: Quat, name: &str) -> Entity {
    let marker = FogVolumeMarker::default();
    commands
        .spawn((
            SceneEntity,
            Name::new(name.to_string()),
            marker.clone(),
            FogVolume {
                fog_color: marker.fog_color,
                density_factor: marker.density_factor,
                absorption: marker.absorption,
                scattering: marker.scattering,
                scattering_asymmetry: marker.scattering_asymmetry,
                light_tint: marker.light_tint,
                light_intensity: marker.light_intensity,
                ..default()
            },
            // Default scale of 10 units for the fog volume bounding box
            Transform::from_translation(position).with_rotation(rotation).with_scale(Vec3::splat(10.0)),
            Visibility::default(),
            // Small collider for selection (fog volume size is determined by transform scale)
            Collider::cuboid(0.5, 0.5, 0.5),
        ))
        .id()
}

fn handle_parent_to_group(
    mut events: MessageReader<ParentToGroupEvent>,
    mut commands: Commands,
    groups: Query<Entity, With<GroupMarker>>,
) {
    for event in events.read() {
        // Verify the parent is a valid group
        if groups.get(event.parent).is_ok() {
            commands.entity(event.child).set_parent_in_place(event.parent);
            info!("Parented entity to group");
        } else {
            warn!("Target entity is not a group");
        }
    }
}

fn handle_unparent(mut events: MessageReader<UnparentEvent>, mut commands: Commands) {
    for event in events.read() {
        commands.entity(event.entity).remove_parent_in_place();
        info!("Unparented entity");
    }
}

fn handle_unparent_selected(
    mut events: MessageReader<UnparentSelectedEvent>,
    mut commands: Commands,
    selected: Query<Entity, With<Selected>>,
) {
    for _ in events.read() {
        for entity in selected.iter() {
            commands.entity(entity).remove_parent_in_place();
        }
        if !selected.is_empty() {
            info!("Unparented selected entities");
        }
    }
}

fn handle_group_selected(
    mut events: MessageReader<GroupSelectedEvent>,
    mut commands: Commands,
    selected: Query<Entity, With<Selected>>,
    existing_entities: Query<&Name, With<SceneEntity>>,
) {
    for _ in events.read() {
        let selected_entities: Vec<_> = selected.iter().collect();

        // Need at least 2 entities to group
        if selected_entities.len() < 2 {
            info!("Select at least 2 entities to create a group");
            return;
        }

        // Create the group with identity transform
        let name = generate_unique_name("Group", &existing_entities);
        let group_entity = commands
            .spawn((
                SceneEntity,
                GroupMarker,
                Name::new(name.clone()),
                Transform::default(),
                Visibility::default(),
            ))
            .id();

        // Parent all selected entities to the group
        for entity in selected_entities {
            commands.entity(entity).set_parent_in_place(group_entity);
        }

        info!("Created group '{}' with {} entities", name, selected.iter().count());
    }
}

/// Collider radius for light selection (small sphere for clicking)
/// Re-exported from constants for backwards compatibility
pub const LIGHT_COLLIDER_RADIUS: f32 = physics::LIGHT_COLLIDER_RADIUS;
