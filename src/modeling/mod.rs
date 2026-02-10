//! Mesh modeling tool — face selection, extrusion, and cutting.
//!
//! Replaces the old Blockout mode (B key). Lets you select surface regions
//! on any mesh entity using grid or freeform modes, then extrude or cut.

pub mod cut;
pub mod edit_mesh;
pub mod extrude;
mod gizmos;
mod input;
pub mod marker;
pub mod selection;

use bevy::prelude::*;
use std::collections::HashSet;

use crate::editor::EditorMode;
use crate::selection::Selected;

use edit_mesh::{EditMesh, FaceIndex};
use gizmos::draw_model_gizmos;
use input::{handle_extrude_drag, handle_model_click, handle_model_confirm, handle_model_input};
use marker::EditMeshMarker;

/// Grid projection type for face selection.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum GridType {
    /// Quantize face centers into world-space axis-aligned cells.
    #[default]
    WorldSpace,
    /// Flood-fill from clicked face, grouping by normal similarity.
    SurfaceSpace,
    /// Quantize face centers into UV-space cells.
    UVSpace,
    /// User draws a polygon on the surface; faces inside are selected.
    Freeform,
}

impl GridType {
    pub fn display_name(&self) -> &'static str {
        match self {
            GridType::WorldSpace => "World",
            GridType::SurfaceSpace => "Surface",
            GridType::UVSpace => "UV",
            GridType::Freeform => "Freeform",
        }
    }
}

/// Active modeling operation.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum ModelOperation {
    /// Selecting faces (default).
    #[default]
    Select,
    /// Extrude selected faces outward.
    Extrude,
    /// Cut selected faces into a separate entity.
    Cut,
}

impl ModelOperation {
    pub fn display_name(&self) -> &'static str {
        match self {
            ModelOperation::Select => "Select",
            ModelOperation::Extrude => "Extrude",
            ModelOperation::Cut => "Cut",
        }
    }
}

/// Persistent state for the mesh modeling tool.
#[derive(Resource)]
pub struct MeshModelState {
    /// Current grid/selection mode.
    pub grid_type: GridType,
    /// World-space grid cell size (default 0.5).
    pub world_grid_size: f32,
    /// UV-space grid cell size (default 0.1).
    pub uv_grid_size: f32,
    /// Angle threshold in degrees for surface-space flood-fill (default 30).
    pub surface_angle_threshold: f32,
    /// Currently selected face indices.
    pub selected_faces: HashSet<FaceIndex>,
    /// The entity being edited.
    pub target_entity: Option<Entity>,
    /// In-memory mesh representation of the target entity.
    pub edit_mesh: Option<EditMesh>,
    /// Current operation (select, extrude, cut).
    pub pending_operation: ModelOperation,
    /// Extrusion distance (adjusted by mouse drag or panel slider).
    pub extrude_distance: f32,
    /// Extrusion tilt angle in degrees.
    pub extrude_angle: f32,
    /// In-progress freeform polygon vertices (world space).
    pub freeform_points: Vec<Vec3>,
    /// Whether we are currently drawing a freeform polygon.
    pub drawing_freeform: bool,
    /// When false (default), face picking only selects front-facing triangles.
    /// When true, picks through the mesh (selects backfaces too).
    pub xray_selection: bool,
    /// World-space extrude origin (set when drag starts).
    pub extrude_drag_origin: Option<Vec3>,
    /// World-space extrude normal direction (set when drag starts).
    pub extrude_drag_normal: Option<Vec3>,
    /// Baseline distance at drag start (so dragging is relative).
    pub extrude_drag_baseline: f32,
}

impl Default for MeshModelState {
    fn default() -> Self {
        Self {
            grid_type: GridType::default(),
            world_grid_size: 0.5,
            uv_grid_size: 0.1,
            surface_angle_threshold: 30.0,
            selected_faces: HashSet::new(),
            target_entity: None,
            edit_mesh: None,
            pending_operation: ModelOperation::default(),
            extrude_distance: 0.0,
            extrude_angle: 0.0,
            freeform_points: Vec::new(),
            drawing_freeform: false,
            xray_selection: false,
            extrude_drag_origin: None,
            extrude_drag_normal: None,
            extrude_drag_baseline: 0.0,
        }
    }
}

impl MeshModelState {
    pub fn reset(&mut self) {
        self.selected_faces.clear();
        self.target_entity = None;
        self.edit_mesh = None;
        self.pending_operation = ModelOperation::Select;
        self.extrude_distance = 0.0;
        self.extrude_angle = 0.0;
        self.freeform_points.clear();
        self.drawing_freeform = false;
        self.extrude_drag_origin = None;
        self.extrude_drag_normal = None;
        self.extrude_drag_baseline = 0.0;
    }
}

pub struct MeshModelPlugin;

impl Plugin for MeshModelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MeshModelState>()
            .register_type::<EditMeshMarker>()
            .add_systems(OnEnter(EditorMode::Blockout), on_enter_model_mode)
            .add_systems(OnExit(EditorMode::Blockout), on_exit_model_mode)
            .add_systems(
                Update,
                (
                    handle_model_input,
                    handle_model_click,
                    handle_model_confirm,
                    handle_extrude_drag,
                    sync_target_entity,
                )
                    .run_if(in_state(EditorMode::Blockout)),
            )
            .add_systems(
                Update,
                draw_model_gizmos.run_if(in_state(EditorMode::Blockout)),
            );
    }
}

/// When entering Model mode, lock onto the currently selected entity.
fn on_enter_model_mode(
    mut model_state: ResMut<MeshModelState>,
    selected: Query<(Entity, &Mesh3d), With<Selected>>,
    meshes: Res<Assets<Mesh>>,
) {
    // Use first selected entity with a mesh as target
    if let Some((entity, mesh_handle)) = selected.iter().next() {
        model_state.target_entity = Some(entity);

        // Try to load the mesh into EditMesh
        if let Some(mesh) = meshes.get(&mesh_handle.0) {
            if let Some(edit_mesh) = EditMesh::from_bevy_mesh(mesh) {
                model_state.edit_mesh = Some(edit_mesh);
            }
        }

        info!("Entered Model mode — target entity {:?}", entity);
    } else {
        info!("Entered Model mode — no mesh entity selected");
    }
}

/// Clean up when exiting Model mode.
fn on_exit_model_mode(mut model_state: ResMut<MeshModelState>) {
    model_state.reset();
    info!("Exited Model mode");
}

/// Keep target_entity in sync with selection changes while in Model mode.
fn sync_target_entity(
    mut model_state: ResMut<MeshModelState>,
    selected: Query<(Entity, &Mesh3d), With<Selected>>,
    meshes: Res<Assets<Mesh>>,
) {
    // If target entity is no longer selected, update to current selection
    if let Some(target) = model_state.target_entity {
        if selected.get(target).is_err() {
            // Target lost — try to pick up a new one
            if let Some((entity, mesh_handle)) = selected.iter().next() {
                model_state.target_entity = Some(entity);
                model_state.selected_faces.clear();
                if let Some(mesh) = meshes.get(&mesh_handle.0) {
                    model_state.edit_mesh = EditMesh::from_bevy_mesh(mesh);
                }
            } else {
                model_state.target_entity = None;
                model_state.edit_mesh = None;
                model_state.selected_faces.clear();
            }
        }
    } else {
        // No target — try to acquire from selection
        if let Some((entity, mesh_handle)) = selected.iter().next() {
            model_state.target_entity = Some(entity);
            if let Some(mesh) = meshes.get(&mesh_handle.0) {
                model_state.edit_mesh = EditMesh::from_bevy_mesh(mesh);
            }
        }
    }
}
