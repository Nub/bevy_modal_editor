//! Mesh modeling tool — face selection, extrusion, and cutting.
//!
//! Replaces the old Blockout mode (B key). Lets you select surface regions
//! on any mesh entity using grid or freeform modes, then extrude or cut.

pub mod bevel;
pub mod boolean;
pub mod bridge;
pub mod cut;
pub mod delete;
pub mod edit_mesh;
pub mod edge_loop;
pub mod extrude;
pub mod fill_hole;
mod gizmos;
pub mod half_edge;
pub mod inset;
mod input;
pub mod marker;
pub mod mirror;
pub mod plane_cut;
pub mod push_pull;
pub mod remesh;
pub mod selection;
pub mod simplify;
pub mod catmull_clark;
pub mod normals;
pub mod select_ops;
pub mod snap;
pub mod smooth;
pub mod soft_select;
pub mod uv_project;
pub mod uv_seam;
pub mod uv_unwrap;
pub mod weld;

use bevy::prelude::*;
use std::collections::HashSet;

use edit_mesh::Edge;
use snap::SnapMode;
use soft_select::FalloffCurve;
use uv_project::{ProjectionAxis, UvProjection};
use uv_seam::SeamEdge;

use crate::editor::EditorMode;
use crate::selection::Selected;

use edit_mesh::{EditMesh, FaceIndex};
use gizmos::draw_model_gizmos;
use half_edge::HalfEdgeMesh;
use input::{handle_extrude_drag, handle_model_click, handle_model_confirm, handle_model_delete, handle_model_input};
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
    /// Selecting elements (default).
    #[default]
    Select,
    /// Extrude selected faces outward.
    Extrude,
    /// Cut selected faces into a separate entity.
    Cut,
    /// Inset selected faces (shrink inward, bridge to original boundary).
    Inset,
    /// Bevel selected edges (split into parallel edges with connecting strip).
    Bevel,
    /// Push/pull faces along individual normals (no side walls).
    PushPull,
    /// Bridge two boundary edge loops with quad strips.
    Bridge,
    /// Weld (merge) nearby vertices.
    Weld,
    /// Insert an edge loop across the mesh.
    EdgeLoop,
    // -- Phase 3: Mesh utilities --
    /// Mirror mesh across an axis plane.
    Mirror,
    /// Laplacian smoothing.
    Smooth,
    /// Midpoint subdivision.
    Subdivide,
    /// Fill boundary holes.
    FillHoles,
    /// Slice mesh with a plane.
    PlaneCut,
    /// QEM-based mesh simplification.
    Simplify,
    /// Uniform triangle remeshing.
    Remesh,
    /// CSG boolean (union/subtract/intersect with another mesh).
    Boolean,
    // -- Phase 4: UV tools --
    /// Apply UV projection (box/planar/cylindrical).
    UvProject,
    /// Unwrap UVs along seam edges.
    UvUnwrap,
    // -- Phase 5: Polish --
    /// Auto-smooth normals by angle threshold.
    AutoSmooth,
    /// Flat normals (split all vertices per face).
    FlatNormals,
    /// Catmull-Clark subdivision.
    CatmullClark,
    /// Snap selected vertices to grid.
    SnapToGrid,
}

impl ModelOperation {
    pub fn display_name(&self) -> &'static str {
        match self {
            ModelOperation::Select => "Select",
            ModelOperation::Extrude => "Extrude",
            ModelOperation::Cut => "Cut",
            ModelOperation::Inset => "Inset",
            ModelOperation::Bevel => "Bevel",
            ModelOperation::PushPull => "Push/Pull",
            ModelOperation::Bridge => "Bridge",
            ModelOperation::Weld => "Weld",
            ModelOperation::EdgeLoop => "Edge Loop",
            ModelOperation::Mirror => "Mirror",
            ModelOperation::Smooth => "Smooth",
            ModelOperation::Subdivide => "Subdivide",
            ModelOperation::FillHoles => "Fill Holes",
            ModelOperation::PlaneCut => "Plane Cut",
            ModelOperation::Simplify => "Simplify",
            ModelOperation::Remesh => "Remesh",
            ModelOperation::Boolean => "Boolean",
            ModelOperation::UvProject => "UV Project",
            ModelOperation::UvUnwrap => "UV Unwrap",
            ModelOperation::AutoSmooth => "Auto Smooth",
            ModelOperation::FlatNormals => "Flat Normals",
            ModelOperation::CatmullClark => "Catmull-Clark",
            ModelOperation::SnapToGrid => "Snap to Grid",
        }
    }

    /// Whether this operation applies to the whole mesh (not a selection).
    pub fn is_whole_mesh_op(&self) -> bool {
        matches!(
            self,
            ModelOperation::Mirror
                | ModelOperation::Smooth
                | ModelOperation::Subdivide
                | ModelOperation::FillHoles
                | ModelOperation::PlaneCut
                | ModelOperation::Simplify
                | ModelOperation::Remesh
                | ModelOperation::Boolean
                | ModelOperation::UvProject
                | ModelOperation::UvUnwrap
                | ModelOperation::AutoSmooth
                | ModelOperation::FlatNormals
                | ModelOperation::CatmullClark
                | ModelOperation::SnapToGrid
        )
    }
}

/// Element selection mode: what kind of mesh element clicks select.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum SelectionMode {
    /// Select individual vertices.
    Vertex,
    /// Select edges.
    Edge,
    /// Select triangle faces (default).
    #[default]
    Face,
}

impl SelectionMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            SelectionMode::Vertex => "Vertex",
            SelectionMode::Edge => "Edge",
            SelectionMode::Face => "Face",
        }
    }

    pub fn key_hint(&self) -> &'static str {
        match self {
            SelectionMode::Vertex => "A",
            SelectionMode::Edge => "S",
            SelectionMode::Face => "D",
        }
    }
}

/// Selected mesh elements — vertices, edges, or faces.
#[derive(Debug, Clone, Default)]
pub enum ElementSelection {
    #[default]
    None,
    /// Selected vertex indices.
    Vertices(HashSet<u32>),
    /// Selected half-edge indices (each represents a geometric edge).
    Edges(HashSet<u32>),
    /// Selected face indices.
    Faces(HashSet<usize>),
}

impl ElementSelection {
    pub fn is_empty(&self) -> bool {
        match self {
            ElementSelection::None => true,
            ElementSelection::Vertices(v) => v.is_empty(),
            ElementSelection::Edges(e) => e.is_empty(),
            ElementSelection::Faces(f) => f.is_empty(),
        }
    }

    pub fn count(&self) -> usize {
        match self {
            ElementSelection::None => 0,
            ElementSelection::Vertices(v) => v.len(),
            ElementSelection::Edges(e) => e.len(),
            ElementSelection::Faces(f) => f.len(),
        }
    }

    pub fn clear(&mut self) {
        *self = ElementSelection::None;
    }

    /// Get as face set (for backward compatibility with existing face-based operations).
    pub fn as_faces(&self) -> Option<&HashSet<usize>> {
        match self {
            ElementSelection::Faces(f) => Some(f),
            _ => None,
        }
    }

    /// Get as edge set.
    pub fn as_edges(&self) -> Option<&HashSet<u32>> {
        match self {
            ElementSelection::Edges(e) => Some(e),
            _ => None,
        }
    }

    /// Get as vertex set.
    pub fn as_vertices(&self) -> Option<&HashSet<u32>> {
        match self {
            ElementSelection::Vertices(v) => Some(v),
            _ => None,
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
    /// Currently selected face indices (legacy — used when selection_mode == Face).
    pub selected_faces: HashSet<FaceIndex>,
    /// The entity being edited.
    pub target_entity: Option<Entity>,
    /// In-memory mesh representation of the target entity.
    pub edit_mesh: Option<EditMesh>,
    /// Half-edge mesh built from edit_mesh for topology operations.
    pub half_edge_mesh: Option<HalfEdgeMesh>,
    /// Current element selection mode (vertex/edge/face).
    pub selection_mode: SelectionMode,
    /// Currently selected elements (typed by selection mode).
    pub element_selection: ElementSelection,
    /// Current operation (select, extrude, cut, inset, bevel).
    pub pending_operation: ModelOperation,
    /// Extrusion distance (adjusted by mouse drag or panel slider).
    pub extrude_distance: f32,
    /// Extrusion tilt angle in degrees.
    pub extrude_angle: f32,
    /// Inset distance (0.0 to 1.0, fraction toward centroid).
    pub inset_distance: f32,
    /// Bevel width (offset distance for bevel operation).
    pub bevel_width: f32,
    /// Push/pull distance along face normals.
    pub push_pull_distance: f32,
    /// Weld threshold distance for merging vertices.
    pub weld_threshold: f32,
    /// In-progress freeform polygon vertices (world space).
    pub freeform_points: Vec<Vec3>,
    /// Whether we are currently drawing a freeform polygon.
    pub drawing_freeform: bool,
    /// When false (default), face picking only selects front-facing triangles.
    /// When true, picks through the mesh (selects backfaces too).
    pub xray_selection: bool,
    /// Screen-space cursor position when extrude drag started.
    pub extrude_drag_screen_start: Option<Vec2>,
    /// Screen-space direction of the extrude normal (normalized).
    pub extrude_drag_screen_dir: Option<Vec2>,
    /// Pixels per world-unit along the extrude normal (for screen→world conversion).
    pub extrude_drag_pixels_per_unit: f32,
    /// Set by input when Delete/Backspace is pressed; consumed by confirm system.
    pub delete_requested: bool,
    /// Set by UI panel buttons; consumed by confirm system.
    pub confirm_requested: bool,
    // -- Phase 3 parameters --
    /// Mirror axis for mirror operation.
    pub mirror_axis: mirror::MirrorAxis,
    /// Smoothing iterations.
    pub smooth_iterations: u32,
    /// Smoothing factor (0–1).
    pub smooth_factor: f32,
    /// Target ratio for simplification (0–1, fraction of original triangle count).
    pub simplify_ratio: f32,
    /// Target edge length for remeshing.
    pub remesh_edge_length: f32,
    /// Plane cut normal axis.
    pub plane_cut_axis: mirror::MirrorAxis,
    /// Boolean operation type.
    pub boolean_op: boolean::BooleanOp,
    /// Second entity for boolean operations.
    pub boolean_target: Option<Entity>,
    // -- Phase 4: UV parameters --
    /// UV seam edges (canonical vertex pairs).
    pub uv_seams: HashSet<SeamEdge>,
    /// UV projection method.
    pub uv_projection: UvProjection,
    /// UV projection axis.
    pub uv_projection_axis: ProjectionAxis,
    /// UV projection scale.
    pub uv_projection_scale: f32,
    /// Whether to show the UV editor panel.
    pub show_uv_editor: bool,
    // -- Phase 5: Polish parameters --
    /// Hard edges for normal splitting.
    pub hard_edges: HashSet<Edge>,
    /// Auto-smooth angle threshold in degrees.
    pub auto_smooth_angle: f32,
    /// Active snap mode.
    pub snap_mode: SnapMode,
    /// Snap grid size.
    pub snap_grid_size: f32,
    /// Soft selection enabled.
    pub soft_selection: bool,
    /// Soft selection radius.
    pub soft_radius: f32,
    /// Soft selection falloff curve.
    pub soft_falloff: FalloffCurve,
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
            half_edge_mesh: None,
            selection_mode: SelectionMode::default(),
            element_selection: ElementSelection::default(),
            pending_operation: ModelOperation::default(),
            extrude_distance: 0.0,
            extrude_angle: 0.0,
            inset_distance: 0.2,
            bevel_width: 0.1,
            push_pull_distance: 0.0,
            weld_threshold: 0.01,
            freeform_points: Vec::new(),
            drawing_freeform: false,
            xray_selection: false,
            extrude_drag_screen_start: None,
            extrude_drag_screen_dir: None,
            extrude_drag_pixels_per_unit: 1.0,
            delete_requested: false,
            confirm_requested: false,
            mirror_axis: mirror::MirrorAxis::X,
            smooth_iterations: 3,
            smooth_factor: 0.5,
            simplify_ratio: 0.5,
            remesh_edge_length: 0.25,
            plane_cut_axis: mirror::MirrorAxis::X,
            boolean_op: boolean::BooleanOp::Union,
            boolean_target: None,
            uv_seams: HashSet::new(),
            uv_projection: UvProjection::Box,
            uv_projection_axis: ProjectionAxis::Y,
            uv_projection_scale: 1.0,
            show_uv_editor: false,
            hard_edges: HashSet::new(),
            auto_smooth_angle: 30.0,
            snap_mode: SnapMode::None,
            snap_grid_size: 0.25,
            soft_selection: false,
            soft_radius: 1.0,
            soft_falloff: FalloffCurve::Smooth,
        }
    }
}

impl MeshModelState {
    pub fn reset(&mut self) {
        self.selected_faces.clear();
        self.target_entity = None;
        self.edit_mesh = None;
        self.half_edge_mesh = None;
        self.selection_mode = SelectionMode::Face;
        self.element_selection.clear();
        self.pending_operation = ModelOperation::Select;
        self.extrude_distance = 0.0;
        self.extrude_angle = 0.0;
        self.inset_distance = 0.2;
        self.bevel_width = 0.1;
        self.push_pull_distance = 0.0;
        self.weld_threshold = 0.01;
        self.freeform_points.clear();
        self.drawing_freeform = false;
        self.extrude_drag_screen_start = None;
        self.extrude_drag_screen_dir = None;
        self.extrude_drag_pixels_per_unit = 1.0;
        self.delete_requested = false;
        self.confirm_requested = false;
        self.mirror_axis = mirror::MirrorAxis::X;
        self.smooth_iterations = 3;
        self.smooth_factor = 0.5;
        self.simplify_ratio = 0.5;
        self.remesh_edge_length = 0.25;
        self.plane_cut_axis = mirror::MirrorAxis::X;
        self.boolean_op = boolean::BooleanOp::Union;
        self.boolean_target = None;
        self.uv_seams.clear();
        self.uv_projection = UvProjection::Box;
        self.uv_projection_axis = ProjectionAxis::Y;
        self.uv_projection_scale = 1.0;
        self.show_uv_editor = false;
        self.hard_edges.clear();
        self.auto_smooth_angle = 30.0;
        self.snap_mode = SnapMode::None;
        self.snap_grid_size = 0.25;
        self.soft_selection = false;
        self.soft_radius = 1.0;
        self.soft_falloff = FalloffCurve::Smooth;
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
                    handle_model_delete,
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

        // Try to load the mesh into EditMesh, then build HalfEdgeMesh
        if let Some(mesh) = meshes.get(&mesh_handle.0) {
            if let Some(edit_mesh) = EditMesh::from_bevy_mesh(mesh) {
                let he_mesh = HalfEdgeMesh::from_edit_mesh(&edit_mesh);
                model_state.half_edge_mesh = Some(he_mesh);
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
    // Helper: load edit mesh + half-edge mesh from a Bevy mesh handle
    fn load_mesh(
        model_state: &mut MeshModelState,
        mesh_handle: &Mesh3d,
        meshes: &Assets<Mesh>,
    ) {
        if let Some(mesh) = meshes.get(&mesh_handle.0) {
            if let Some(edit_mesh) = EditMesh::from_bevy_mesh(mesh) {
                let he_mesh = HalfEdgeMesh::from_edit_mesh(&edit_mesh);
                model_state.half_edge_mesh = Some(he_mesh);
                model_state.edit_mesh = Some(edit_mesh);
            } else {
                model_state.edit_mesh = None;
                model_state.half_edge_mesh = None;
            }
        }
    }

    // If target entity is no longer selected, update to current selection
    if let Some(target) = model_state.target_entity {
        if selected.get(target).is_err() {
            // Target lost — try to pick up a new one
            if let Some((entity, mesh_handle)) = selected.iter().next() {
                model_state.target_entity = Some(entity);
                model_state.selected_faces.clear();
                model_state.element_selection.clear();
                load_mesh(&mut model_state, mesh_handle, &meshes);
            } else {
                model_state.target_entity = None;
                model_state.edit_mesh = None;
                model_state.half_edge_mesh = None;
                model_state.selected_faces.clear();
                model_state.element_selection.clear();
            }
        }
    } else {
        // No target — try to acquire from selection
        if let Some((entity, mesh_handle)) = selected.iter().next() {
            model_state.target_entity = Some(entity);
            load_mesh(&mut model_state, mesh_handle, &meshes);
        }
    }
}
