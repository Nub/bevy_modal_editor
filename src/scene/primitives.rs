use avian3d::prelude::*;
use bevy::light::FogVolume;
use bevy::prelude::*;
use bevy_spline_3d::prelude::{Spline, SplineType};
use serde::{Deserialize, Serialize};

use bevy::pbr::ExtendedMaterial;
use bevy_editor_game::{BaseMaterialProps, CustomEntityRegistry, MaterialDefinition, MaterialRef};
use bevy_grid_shader::GridMaterial;

use super::blockout::{spawn_arch, spawn_lshape, spawn_ramp, spawn_stairs, GridMat};
use super::SceneEntity;
use crate::commands::TakeSnapshotCommand;
use crate::constants::{light_colors, physics, primitive_colors};
use crate::materials::grid::GridMaterialProps;
use crate::effects::{EffectLibrary, EffectMarker};
use crate::particles::ParticleEffectMarker;
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
    #[serde(default)]
    pub radius: f32,
}

impl Default for SceneLightMarker {
    fn default() -> Self {
        Self {
            color: light_colors::POINT_DEFAULT,
            intensity: light_colors::POINT_DEFAULT_INTENSITY,
            range: light_colors::POINT_DEFAULT_RANGE,
            shadows_enabled: true,
            radius: 0.0,
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

/// Serializable material properties for primitive entities.
///
/// Stores the base_color so it survives scene snapshot/restore cycles.
/// Without this, `regenerate_runtime_components` would always use the shape's
/// default color, losing any custom colors set by the game or user.
#[derive(Component, Serialize, Deserialize, Clone, Reflect)]
#[reflect(Component)]
pub struct PrimitiveMaterial {
    pub base_color: Color,
}

impl PrimitiveMaterial {
    pub fn new(base_color: Color) -> Self {
        Self { base_color }
    }
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

    /// Create the mesh for this primitive shape (with tangents for normal mapping)
    pub fn create_mesh(&self) -> Mesh {
        let mesh = match self {
            PrimitiveShape::Cube => Mesh::from(Cuboid::new(1.0, 1.0, 1.0)),
            PrimitiveShape::Sphere => Mesh::from(Sphere::new(0.5)),
            PrimitiveShape::Cylinder => Mesh::from(Cylinder::new(0.5, 1.0)),
            PrimitiveShape::Capsule => Mesh::from(Capsule3d::new(0.25, 0.5)),
            PrimitiveShape::Plane => Plane3d::default().mesh().size(2.0, 2.0).build(),
        };
        mesh.with_generated_tangents().expect("primitive mesh should support tangent generation")
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
            PrimitiveShape::Cylinder => Collider::cylinder(0.5, 1.0),
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
    /// A particle effect (bevy_hanabi)
    ParticleEffect,
    /// A particle effect from a named preset
    ParticlePreset(String),
    /// An effect (effect sequencer)
    Effect,
    /// An effect from a named preset
    EffectPreset(String),
    /// A custom entity type registered by the game
    Custom(String),
}

impl SpawnEntityKind {
    /// Get the display name for this entity kind
    pub fn display_name(&self) -> String {
        match self {
            SpawnEntityKind::Primitive(shape) => shape.display_name().to_string(),
            SpawnEntityKind::Group => "Group".to_string(),
            SpawnEntityKind::PointLight => "Point Light".to_string(),
            SpawnEntityKind::DirectionalLight => "Sun".to_string(),
            SpawnEntityKind::Spline(spline_type) => match spline_type {
                SplineType::CubicBezier => "Bezier Spline",
                SplineType::CatmullRom => "Catmull-Rom Spline",
                SplineType::BSpline => "B-Spline",
            }
            .to_string(),
            SpawnEntityKind::FogVolume => "Fog Volume".to_string(),
            SpawnEntityKind::Stairs => "Stairs".to_string(),
            SpawnEntityKind::Ramp => "Ramp".to_string(),
            SpawnEntityKind::Arch => "Arch".to_string(),
            SpawnEntityKind::LShape => "L-Shape".to_string(),
            SpawnEntityKind::ParticleEffect => "Particle Effect".to_string(),
            SpawnEntityKind::ParticlePreset(name) => format!("Particle: {}", name),
            SpawnEntityKind::Effect => "Effect".to_string(),
            SpawnEntityKind::EffectPreset(name) => format!("Effect: {}", name),
            SpawnEntityKind::Custom(name) => name.clone(),
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
    mut grid_materials: ResMut<Assets<GridMat>>,
    existing_entities: Query<&Name, With<SceneEntity>>,
    selected_entities: Query<Entity, With<Selected>>,
    custom_registry: Res<CustomEntityRegistry>,
    particle_library: Res<crate::particles::ParticleLibrary>,
    effect_library: Res<EffectLibrary>,
) {
    for event in events.read() {
        // Deselect all currently selected entities
        for entity in selected_entities.iter() {
            commands.entity(entity).remove::<Selected>();
        }

        let display = event.kind.display_name();
        let name = generate_unique_name(&display, &existing_entities);

        let new_entity = match &event.kind {
            SpawnEntityKind::Primitive(shape) => spawn_primitive(
                &mut commands,
                &mut meshes,
                &mut grid_materials,
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
            SpawnEntityKind::Stairs => spawn_stairs(&mut commands, &mut meshes, &mut grid_materials, event.position, event.rotation, &name),
            SpawnEntityKind::Ramp => spawn_ramp(&mut commands, &mut meshes, &mut grid_materials, event.position, event.rotation, &name),
            SpawnEntityKind::Arch => spawn_arch(&mut commands, &mut meshes, &mut grid_materials, event.position, event.rotation, &name),
            SpawnEntityKind::LShape => spawn_lshape(&mut commands, &mut meshes, &mut grid_materials, event.position, event.rotation, &name),
            SpawnEntityKind::ParticleEffect => spawn_particle_effect(&mut commands, event.position, event.rotation, &name),
            SpawnEntityKind::ParticlePreset(preset_name) => {
                let marker = particle_library
                    .effects
                    .get(preset_name)
                    .cloned()
                    .unwrap_or_default();
                spawn_particle_effect_with_marker(&mut commands, event.position, event.rotation, &name, marker)
            }
            SpawnEntityKind::Effect => spawn_effect(&mut commands, event.position, event.rotation, &name, EffectMarker::default()),
            SpawnEntityKind::EffectPreset(preset_name) => {
                let marker = effect_library
                    .effects
                    .get(preset_name)
                    .cloned()
                    .unwrap_or_default();
                spawn_effect(&mut commands, event.position, event.rotation, &name, marker)
            }
            SpawnEntityKind::Custom(type_name) => {
                let entry = custom_registry
                    .entries
                    .iter()
                    .find(|e| e.entity_type.name == type_name)
                    .unwrap_or_else(|| panic!("Unknown custom entity type: {}", type_name));
                let entity = (entry.entity_type.spawn)(&mut commands, event.position, event.rotation);
                commands.entity(entity).insert((SceneEntity, Name::new(name.clone())));
                entity
            }
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
    materials: &mut ResMut<Assets<GridMat>>,
    shape: PrimitiveShape,
    position: Vec3,
    rotation: Quat,
    name: &str,
) -> Entity {
    let color = shape.default_color();
    let grid_data =
        ron::to_string(&GridMaterialProps::default()).unwrap_or_default();
    commands
        .spawn((
            SceneEntity,
            Name::new(name.to_string()),
            PrimitiveMarker { shape },
            MaterialRef::Inline(MaterialDefinition::with_extension(
                BaseMaterialProps {
                    base_color: color,
                    ..default()
                },
                "grid",
                grid_data,
            )),
            Mesh3d(meshes.add(shape.create_mesh())),
            MeshMaterial3d(materials.add(ExtendedMaterial {
                base: StandardMaterial {
                    base_color: color,
                    ..default()
                },
                extension: GridMaterial::default(),
            })),
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
                radius: light_marker.radius,
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

/// Spawn a particle effect container entity with default settings.
/// The `ParticleEffectMarker` is serializable; a disposable child entity
/// holding the actual `ParticleEffect` is spawned by `ParticlePlugin`.
pub fn spawn_particle_effect(commands: &mut Commands, position: Vec3, rotation: Quat, name: &str) -> Entity {
    spawn_particle_effect_with_marker(commands, position, rotation, name, ParticleEffectMarker::default())
}

/// Spawn a particle effect container entity with a specific marker configuration.
pub fn spawn_particle_effect_with_marker(
    commands: &mut Commands,
    position: Vec3,
    rotation: Quat,
    name: &str,
    marker: ParticleEffectMarker,
) -> Entity {
    commands
        .spawn((
            SceneEntity,
            Name::new(name.to_string()),
            marker,
            Transform::from_translation(position).with_rotation(rotation),
            Visibility::default(),
            Collider::sphere(physics::LIGHT_COLLIDER_RADIUS),
        ))
        .id()
}

/// Spawn an effect container entity with a specific marker configuration.
pub fn spawn_effect(
    commands: &mut Commands,
    position: Vec3,
    rotation: Quat,
    name: &str,
    marker: EffectMarker,
) -> Entity {
    commands
        .spawn((
            SceneEntity,
            Name::new(name.to_string()),
            marker,
            Transform::from_translation(position).with_rotation(rotation),
            Visibility::default(),
            Collider::sphere(physics::LIGHT_COLLIDER_RADIUS),
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
            commands.queue(TakeSnapshotCommand {
                description: "Parent to group".to_string(),
            });
            commands.entity(event.child).set_parent_in_place(event.parent);
            info!("Parented entity to group");
        } else {
            warn!("Target entity is not a group");
        }
    }
}

fn handle_unparent(mut events: MessageReader<UnparentEvent>, mut commands: Commands) {
    for event in events.read() {
        commands.queue(TakeSnapshotCommand {
            description: "Unparent entity".to_string(),
        });
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
        if !selected.is_empty() {
            commands.queue(TakeSnapshotCommand {
                description: "Unparent selected entities".to_string(),
            });
            for entity in selected.iter() {
                commands.entity(entity).remove_parent_in_place();
            }
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

        commands.queue(TakeSnapshotCommand {
            description: "Group selected entities".to_string(),
        });

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
