//! Navigation module: navmesh generation (rerecast) and pathfinding (landmass).
//!
//! Provides [`NavigationPlugin`] which adds navmesh generation, Avian3D collider
//! integration, and landmass pathfinding to the editor.
//!
//! Uses a custom backend that reads `GlobalTransform` directly instead of Avian's
//! `Position`/`Rotation`, which stay at placeholder values when physics is paused.

use avian3d::prelude::*;
use avian_rerecast::ColliderToTriMesh;
use bevy::prelude::*;
use bevy_landmass::prelude::*;
use bevy_rerecast::prelude::*;
use bevy_rerecast::rerecast::TriMesh;
use landmass_rerecast::LandmassRerecastPlugin;
use bevy_rerecast::rerecast::PolygonNavmesh;

use crate::editor::EditorMode;
use crate::scene::SceneEntity;

/// Marker component for the navmesh scene entity (appears in hierarchy).
#[derive(Component, Default, Reflect)]
#[reflect(Component)]
pub struct NavmeshMarker;

/// Tracks navmesh state for the editor UI.
#[derive(Resource, Clone)]
pub struct NavmeshState {
    /// Handle to the current navmesh asset (None if never generated).
    pub handle: Option<Handle<Navmesh>>,
    /// Whether the navmesh asset is ready (generation complete).
    pub ready: bool,
    /// Whether generation is in progress.
    pub generating: bool,
    /// Agent radius used for generation.
    pub agent_radius: f32,
    /// Agent height used for generation.
    pub agent_height: f32,
    /// Whether to show navmesh wireframe gizmos.
    pub show_wireframe: bool,
    /// Number of polygons in the generated navmesh.
    pub polygon_count: usize,
    /// Entity holding the Archipelago for landmass pathfinding.
    pub archipelago_entity: Option<Entity>,
    /// Entity holding the Island.
    pub island_entity: Option<Entity>,
    /// Scene entity representing the navmesh in the hierarchy.
    pub navmesh_entity: Option<Entity>,
    /// Pre-computed wireframe: each polygon as a closed list of world-space vertices.
    pub wireframe_polygons: Vec<Vec<Vec3>>,
}

impl Default for NavmeshState {
    fn default() -> Self {
        Self {
            handle: None,
            ready: false,
            generating: false,
            agent_radius: 0.6,
            agent_height: 1.8,
            show_wireframe: true,
            polygon_count: 0,
            archipelago_entity: None,
            island_entity: None,
            navmesh_entity: None,
            wireframe_polygons: Vec::new(),
        }
    }
}

/// Message to trigger navmesh generation from the AI panel.
#[derive(Message)]
pub struct GenerateNavmeshEvent;

pub struct NavigationPlugin;

impl Plugin for NavigationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<NavmeshState>()
            .add_message::<GenerateNavmeshEvent>()
            .add_plugins(NavmeshPlugins::default())
            // Custom backend: reads GlobalTransform instead of Position/Rotation.
            // This avoids the stale-placeholder issue when physics is paused.
            .set_navmesh_backend(gt_collider_backend)
            .add_plugins(Landmass3dPlugin::default())
            .add_plugins(LandmassRerecastPlugin::default())
            .add_systems(Update, (handle_generate_navmesh, draw_navmesh_wireframe))
            .add_observer(on_navmesh_ready);
    }
}

/// Navmesh backend that reads `GlobalTransform` instead of Avian's `Position`/`Rotation`.
///
/// When physics is paused, Avian's `PhysicsSystems::Prepare` never runs, so
/// `Position`/`Rotation` stay at placeholder values (origin). This backend
/// bypasses that by reading from `GlobalTransform` directly, which is always
/// correct after `TransformSystems::Propagate`.
fn gt_collider_backend(
    input: In<NavmeshSettings>,
    colliders: Query<(Entity, &Collider, &GlobalTransform, &ColliderOf)>,
    bodies: Query<&RigidBody>,
) -> TriMesh {
    colliders
        .iter()
        .filter_map(|(entity, collider, gt, collider_of)| {
            if input
                .filter
                .as_ref()
                .is_some_and(|entities| !entities.contains(&entity))
            {
                return None;
            }
            let body = bodies.get(collider_of.body).ok()?;
            if !body.is_static() {
                return None;
            }
            let (_, rotation, translation) = gt.to_scale_rotation_translation();
            let subdivisions = 10;
            collider.to_trimesh(Position(translation), Rotation(rotation), subdivisions)
        })
        .fold(TriMesh::default(), |mut acc, t| {
            acc.extend(t);
            acc
        })
}

/// Handle the generate navmesh event (triggered from the AI panel).
fn handle_generate_navmesh(
    mut events: MessageReader<GenerateNavmeshEvent>,
    mut generator: NavmeshGenerator,
    mut state: ResMut<NavmeshState>,
) {
    for _ in events.read() {
        let settings = NavmeshSettings::from_agent_3d(state.agent_radius, state.agent_height);

        if let Some(ref handle) = state.handle {
            // Regenerate existing navmesh
            generator.regenerate(handle, settings);
        } else {
            // Generate new navmesh
            let handle = generator.generate(settings);
            state.handle = Some(handle);
        }

        state.generating = true;
        state.ready = false;
        state.wireframe_polygons.clear();
        info!(
            "Navmesh generation started (radius={}, height={})",
            state.agent_radius, state.agent_height
        );
    }
}

/// Draw navmesh wireframe using immediate-mode gizmos (runs every frame).
/// Bypasses bevy_rerecast's retained gizmo pipeline entirely for reliability.
fn draw_navmesh_wireframe(
    mut gizmos: Gizmos,
    state: Res<NavmeshState>,
    mode: Res<State<EditorMode>>,
    editor_state: Res<crate::editor::EditorState>,
) {
    if !state.ready || !state.show_wireframe || !editor_state.editor_active || *mode.get() != EditorMode::AI {
        return;
    }

    let color = Color::srgb(0.22, 0.56, 0.84); // Sky blue, similar to tailwind SKY_700

    for polygon in &state.wireframe_polygons {
        if polygon.len() >= 2 {
            gizmos.linestrip(polygon.clone(), color);
        }
    }
}

/// Extract wireframe polygon data from a navmesh asset into a list of
/// closed world-space vertex loops suitable for `gizmos.linestrip()`.
fn extract_wireframe(navmesh: &Navmesh) -> Vec<Vec<Vec3>> {
    let mesh = &navmesh.polygon;
    let nvp = mesh.max_vertices_per_polygon as usize;
    let origin = mesh.aabb.min;
    let to_local = Vec3::new(mesh.cell_size, mesh.cell_height, mesh.cell_size);

    let mut polygons = Vec::with_capacity(mesh.polygon_count());
    for i in 0..mesh.polygon_count() {
        let poly = &mesh.polygons[i * nvp..];
        let mut verts: Vec<Vec3> = poly[..nvp]
            .iter()
            .filter(|idx| **idx != PolygonNavmesh::NO_INDEX)
            .map(|idx| {
                let vert_local = mesh.vertices[*idx as usize];
                origin + vert_local.as_vec3() * to_local
            })
            .collect();
        if !verts.is_empty() {
            // Close the polygon
            verts.push(verts[0]);
            polygons.push(verts);
        }
    }
    polygons
}

/// Observer for when a navmesh is ready — extracts wireframe, sets up Archipelago + Island.
fn on_navmesh_ready(
    trigger: On<NavmeshReady>,
    navmeshes: Res<Assets<Navmesh>>,
    mut state: ResMut<NavmeshState>,
    mut commands: Commands,
) {
    let asset_id = trigger.event().0;
    let Some(navmesh) = navmeshes.get(asset_id) else {
        warn!("NavmeshReady fired but asset not found");
        return;
    };

    state.generating = false;
    state.ready = true;
    state.polygon_count = navmesh.polygon.polygons.len()
        / navmesh.polygon.max_vertices_per_polygon as usize;

    // Pre-compute wireframe for immediate-mode gizmo drawing
    state.wireframe_polygons = extract_wireframe(navmesh);

    info!(
        "Navmesh ready: {} polygons, {} wireframe outlines",
        state.polygon_count,
        state.wireframe_polygons.len(),
    );

    // Clone handle and radius before mutating state
    let handle = match state.handle.clone() {
        Some(h) => h,
        None => return,
    };
    let agent_radius = state.agent_radius;

    // Ensure a single navmesh entity exists in the scene hierarchy
    if state.navmesh_entity.is_none()
        || state
            .navmesh_entity
            .is_some_and(|e| commands.get_entity(e).is_err())
    {
        let entity = commands
            .spawn((
                SceneEntity,
                NavmeshMarker,
                Name::new("Navmesh"),
                Transform::default(),
            ))
            .id();
        state.navmesh_entity = Some(entity);
    }

    // Spawn or update Archipelago entity
    let archipelago_entity = if let Some(entity) = state.archipelago_entity {
        entity
    } else {
        let options = ArchipelagoOptions::from_agent_radius(agent_radius);
        let entity = commands.spawn(Archipelago3d::new(options)).id();
        state.archipelago_entity = Some(entity);
        entity
    };

    // Spawn or update Island entity with the navmesh
    if let Some(island) = state.island_entity {
        commands.entity(island).insert(
            landmass_rerecast::NavMeshHandle3d(handle),
        );
    } else {
        let island = commands
            .spawn(landmass_rerecast::Island3dBundle {
                island: Island,
                archipelago_ref: ArchipelagoRef3d::new(archipelago_entity),
                nav_mesh: landmass_rerecast::NavMeshHandle3d(handle),
            })
            .id();
        state.island_entity = Some(island);
    }
}
