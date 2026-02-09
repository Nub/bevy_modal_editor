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
use bevy_rerecast::debug::{NavmeshGizmoConfig, PolygonNavmeshGizmo};
use bevy::gizmos::retained::Gizmo;
use bevy_rerecast::prelude::*;
use bevy_rerecast::rerecast::TriMesh;
use landmass_rerecast::LandmassRerecastPlugin;

use crate::editor::EditorMode;

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
    /// Entity holding the PolygonNavmeshGizmo for visualization.
    pub gizmo_entity: Option<Entity>,
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
            gizmo_entity: None,
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
            .add_systems(Update, handle_generate_navmesh)
            .add_observer(on_navmesh_ready)
            .add_systems(Update, (toggle_navmesh_gizmo, debug_gizmo_entity));
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
        info!(
            "Navmesh generation started (radius={}, height={})",
            state.agent_radius, state.agent_height
        );
    }
}

/// Toggle the navmesh polygon gizmo visibility based on editor mode and wireframe setting.
fn toggle_navmesh_gizmo(
    mode: Res<State<EditorMode>>,
    state: Res<NavmeshState>,
    mut config: ResMut<NavmeshGizmoConfig>,
) {
    let should_show = *mode.get() == EditorMode::AI && state.show_wireframe && state.ready;
    if config.polygon_navmesh.enabled != should_show {
        info!(
            "Navmesh gizmo: enabled={} (mode={:?}, wireframe={}, ready={}, gizmo_entity={:?})",
            should_show, mode.get(), state.show_wireframe, state.ready, state.gizmo_entity
        );
        config.polygon_navmesh.enabled = should_show;
    }
}

/// Observer for when a navmesh is ready — sets up the gizmo, Archipelago + Island.
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

    info!(
        "Navmesh ready: {} polygons",
        state.polygon_count,
    );

    // Clone handle and radius before mutating state
    let handle = match state.handle.clone() {
        Some(h) => h,
        None => return,
    };
    let agent_radius = state.agent_radius;

    // Despawn old gizmo if it exists (it may have been despawned by scene reset)
    if let Some(old_gizmo) = state.gizmo_entity.take() {
        commands.entity(old_gizmo).try_despawn();
    }
    // Spawn fresh PolygonNavmeshGizmo entity for visualization
    let gizmo_entity = commands
        .spawn(PolygonNavmeshGizmo::new(asset_id))
        .id();
    state.gizmo_entity = Some(gizmo_entity);

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

fn debug_gizmo_entity(
    state: Res<NavmeshState>,
    gizmos: Query<(Entity, &PolygonNavmeshGizmo, &Visibility, Option<&Gizmo>, Option<&Mesh3d>)>,
    mut ran: Local<bool>,
) {
    if !state.ready || *ran {
        return;
    }
    *ran = true;
    if let Some(entity) = state.gizmo_entity {
        if let Ok((e, png, vis, gizmo, mesh)) = gizmos.get(entity) {
            info!(
                "Gizmo entity {:?}: navmesh_id={:?}, visibility={:?}, has_gizmo={}, has_mesh3d={}",
                e, png.0, vis, gizmo.is_some(), mesh.is_some()
            );
        } else {
            warn!("Gizmo entity {:?} not found in query (missing components?)", entity);
        }
    }
}
