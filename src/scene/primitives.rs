use avian3d::prelude::*;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::SceneEntity;
use crate::selection::Selected;

/// Marker component for group entities (containers for nesting)
#[derive(Component, Serialize, Deserialize, Clone, Default)]
pub struct GroupMarker;

/// Marker component for scene lights (to distinguish from editor lights)
#[derive(Component, Serialize, Deserialize, Clone)]
pub struct SceneLightMarker {
    pub color: Color,
    pub intensity: f32,
    pub range: f32,
    pub shadows_enabled: bool,
}

impl Default for SceneLightMarker {
    fn default() -> Self {
        Self {
            color: Color::WHITE,
            intensity: 100000.0,
            range: 20.0,
            shadows_enabled: true,
        }
    }
}

/// Available primitive shapes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PrimitiveShape {
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
}

/// Event to spawn a primitive shape
#[derive(Message)]
pub struct SpawnPrimitiveEvent {
    pub shape: PrimitiveShape,
    pub position: Vec3,
}

/// Event to spawn an empty group
#[derive(Message)]
pub struct SpawnGroupEvent {
    pub position: Vec3,
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

/// Event to group multiple selected entities into a new group
#[derive(Message)]
pub struct GroupSelectedEvent;

/// Event to spawn a point light
#[derive(Message)]
pub struct SpawnPointLightEvent {
    pub position: Vec3,
}

/// Component to track what primitive shape an entity is
#[derive(Component, Serialize, Deserialize, Clone)]
pub struct PrimitiveMarker {
    pub shape: PrimitiveShape,
}

pub struct PrimitivesPlugin;

impl Plugin for PrimitivesPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<SpawnPrimitiveEvent>()
            .add_message::<SpawnGroupEvent>()
            .add_message::<ParentToGroupEvent>()
            .add_message::<UnparentEvent>()
            .add_message::<GroupSelectedEvent>()
            .add_message::<SpawnPointLightEvent>()
            .add_systems(
                Update,
                (
                    handle_spawn_primitive,
                    handle_spawn_group,
                    handle_parent_to_group,
                    handle_unparent,
                    handle_group_selected,
                    handle_spawn_point_light,
                ),
            );
    }
}

fn handle_spawn_primitive(
    mut events: MessageReader<SpawnPrimitiveEvent>,
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    existing_entities: Query<&Name, With<SceneEntity>>,
) {
    for event in events.read() {
        let name = generate_unique_name(event.shape.display_name(), &existing_entities);

        match event.shape {
            PrimitiveShape::Cube => {
                spawn_cube(&mut commands, &mut meshes, &mut materials, event.position, &name);
            }
            PrimitiveShape::Sphere => {
                spawn_sphere(&mut commands, &mut meshes, &mut materials, event.position, &name);
            }
            PrimitiveShape::Cylinder => {
                spawn_cylinder(&mut commands, &mut meshes, &mut materials, event.position, &name);
            }
            PrimitiveShape::Capsule => {
                spawn_capsule(&mut commands, &mut meshes, &mut materials, event.position, &name);
            }
            PrimitiveShape::Plane => {
                spawn_plane(&mut commands, &mut meshes, &mut materials, event.position, &name);
            }
        }
    }
}

fn generate_unique_name(base: &str, existing: &Query<&Name, With<SceneEntity>>) -> String {
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

fn spawn_cube(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    name: &str,
) {
    commands.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker {
            shape: PrimitiveShape::Cube,
        },
        Mesh3d(meshes.add(Cuboid::new(1.0, 1.0, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.7, 0.6),
            ..default()
        })),
        Transform::from_translation(position),
        RigidBody::Static,
        Collider::cuboid(1.0, 1.0, 1.0),
    ));
}

fn spawn_sphere(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    name: &str,
) {
    commands.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker {
            shape: PrimitiveShape::Sphere,
        },
        Mesh3d(meshes.add(Sphere::new(0.5))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.7, 0.8),
            ..default()
        })),
        Transform::from_translation(position),
        RigidBody::Static,
        Collider::sphere(0.5),
    ));
}

fn spawn_cylinder(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    name: &str,
) {
    commands.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker {
            shape: PrimitiveShape::Cylinder,
        },
        Mesh3d(meshes.add(Cylinder::new(0.5, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.7, 0.8, 0.6),
            ..default()
        })),
        Transform::from_translation(position),
        RigidBody::Static,
        Collider::cylinder(0.5, 0.5),
    ));
}

fn spawn_capsule(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    name: &str,
) {
    commands.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker {
            shape: PrimitiveShape::Capsule,
        },
        Mesh3d(meshes.add(Capsule3d::new(0.25, 0.5))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.8, 0.6, 0.7),
            ..default()
        })),
        Transform::from_translation(position),
        RigidBody::Static,
        Collider::capsule(0.25, 0.5),
    ));
}

fn spawn_plane(
    commands: &mut Commands,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
    position: Vec3,
    name: &str,
) {
    commands.spawn((
        SceneEntity,
        Name::new(name.to_string()),
        PrimitiveMarker {
            shape: PrimitiveShape::Plane,
        },
        Mesh3d(meshes.add(Plane3d::default().mesh().size(2.0, 2.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.6, 0.6, 0.8),
            ..default()
        })),
        Transform::from_translation(position),
        RigidBody::Static,
        Collider::cuboid(2.0, 0.01, 2.0),
    ));
}

fn handle_spawn_group(
    mut events: MessageReader<SpawnGroupEvent>,
    mut commands: Commands,
    existing_entities: Query<&Name, With<SceneEntity>>,
) {
    for event in events.read() {
        let name = generate_unique_name("Group", &existing_entities);

        commands.spawn((
            SceneEntity,
            GroupMarker,
            Name::new(name),
            Transform::from_translation(event.position),
            Visibility::default(),
        ));
    }
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

fn handle_group_selected(
    mut events: MessageReader<GroupSelectedEvent>,
    mut commands: Commands,
    selected: Query<(Entity, &Transform), With<Selected>>,
    existing_entities: Query<&Name, With<SceneEntity>>,
) {
    for _ in events.read() {
        let selected_entities: Vec<_> = selected.iter().collect();

        // Need at least 2 entities to group
        if selected_entities.len() < 2 {
            info!("Select at least 2 entities to create a group");
            return;
        }

        // Calculate center position of selected entities
        let center: Vec3 = selected_entities
            .iter()
            .map(|(_, t)| t.translation)
            .sum::<Vec3>()
            / selected_entities.len() as f32;

        // Create the group
        let name = generate_unique_name("Group", &existing_entities);
        let group_entity = commands
            .spawn((
                SceneEntity,
                GroupMarker,
                Name::new(name.clone()),
                Transform::from_translation(center),
                Visibility::default(),
            ))
            .id();

        // Parent all selected entities to the group
        for (entity, _) in selected_entities {
            commands.entity(entity).set_parent_in_place(group_entity);
        }

        info!("Created group '{}' with {} entities", name, selected.iter().count());
    }
}

fn handle_spawn_point_light(
    mut events: MessageReader<SpawnPointLightEvent>,
    mut commands: Commands,
    existing_entities: Query<&Name, With<SceneEntity>>,
) {
    for event in events.read() {
        let name = generate_unique_name("Point Light", &existing_entities);
        let light_marker = SceneLightMarker::default();

        commands.spawn((
            SceneEntity,
            Name::new(name),
            light_marker.clone(),
            PointLight {
                color: light_marker.color,
                intensity: light_marker.intensity,
                range: light_marker.range,
                shadows_enabled: light_marker.shadows_enabled,
                ..default()
            },
            Transform::from_translation(event.position),
            Visibility::default(),
        ));
    }
}
