use avian3d::debug_render::PhysicsGizmos;
use avian3d::prelude::*;
use avian3d::schedule::PhysicsTime;
use avian3d::spatial_query::SpatialQueryPipeline;
use bevy::gizmos::config::GizmoConfigStore;
use bevy::prelude::*;
use bevy_infinite_grid::InfiniteGridSettings;

use bevy_spline_3d::prelude::SplineType;

use crate::scene::PrimitiveShape;

use bevy_editor_game::{
    GamePausedEvent, GameResetEvent, GameResumedEvent, GameStartedEvent, GameState, PauseEvent,
    PlayEvent, ResetEvent,
};

use super::game::GameSnapshot;

/// The current editor mode (vim-like modal editing)
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, States)]
pub enum EditorMode {
    /// View mode: camera navigation and selection only
    #[default]
    View,
    /// Edit mode: transform manipulation active
    Edit,
    /// Insert mode: adding new objects to the scene
    Insert,
    /// Object Inspector mode: inspect and edit components on selected entity
    ObjectInspector,
    /// Hierarchy mode: shows scene hierarchy panel, '/' searches objects
    Hierarchy,
    /// Blockout mode: keyboard-first tile snapping for rapid prototyping
    Blockout,
    /// Material mode: select and edit materials on selected entity
    Material,
    /// Camera mode: configure render settings (AA, bloom, color grading, etc.)
    Camera,
}

/// The active transform operation in Edit mode
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Resource)]
pub enum TransformOperation {
    #[default]
    None,
    /// Grab/translate (Q key)
    Translate,
    /// Rotate (W key)
    Rotate,
    /// Scale (E key)
    Scale,
    /// Place mode (R key) - raycast-based placement like insert mode
    Place,
    /// Snap to object (T key) - snap position and align rotation to surface
    SnapToObject,
}

/// Axis constraint for transform operations
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Resource)]
pub enum AxisConstraint {
    /// No constraint - free transform
    #[default]
    None,
    /// Constrain to X axis
    X,
    /// Constrain to Y axis
    Y,
    /// Constrain to Z axis
    Z,
}

/// Sub-mode for snap to object operation
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Resource)]
pub enum SnapSubMode {
    /// Align to surface normal (A key) - default
    #[default]
    Surface,
    /// Align centers through AABB (S key)
    Center,
    /// Align using target's rotation - combines surface + center for off-axis objects (D key)
    Aligned,
    /// Snap to nearest vertex of target mesh (F key)
    Vertex,
}

/// Editor-wide state resource
#[derive(Debug, Resource)]
pub struct EditorState {
    /// Whether the editor is active (F10 to toggle)
    /// When false, all UI and hotkeys are disabled
    pub editor_active: bool,
    /// Whether the editor UI is enabled
    pub ui_enabled: bool,
    /// Whether gizmos are visible
    pub gizmos_visible: bool,
    /// Whether distance measurements are visible (M to toggle)
    pub measurements_visible: bool,
    /// Grid snap amount (0.0 = disabled)
    pub grid_snap: f32,
    /// Rotation snap in degrees (0.0 = disabled)
    pub rotation_snap: f32,
}

impl Default for EditorState {
    fn default() -> Self {
        Self {
            editor_active: true,
            ui_enabled: true,
            gizmos_visible: true,
            measurements_visible: false,
            grid_snap: 0.0,
            rotation_snap: 0.0,
        }
    }
}

/// Step amounts for J/K key adjustments
#[derive(Debug, Resource)]
pub struct EditStepAmount {
    /// Translation step in units
    pub translate: f32,
    /// Rotation step in degrees
    pub rotate: f32,
    /// Scale step as multiplier
    pub scale: f32,
}

impl Default for EditStepAmount {
    fn default() -> Self {
        Self {
            translate: 0.5,
            rotate: 15.0,
            scale: 0.1,
        }
    }
}

/// Settings for dimension/edge snapping during translate operations
#[derive(Debug, Resource)]
pub struct DimensionSnapSettings {
    /// Whether edge snapping is enabled
    pub enabled: bool,
    /// Distance threshold for snapping (in world units)
    pub threshold: f32,
}

impl Default for DimensionSnapSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            threshold: 0.5,
        }
    }
}

/// Tracks active edge snaps for visualization
#[derive(Debug, Default, Resource)]
pub struct ActiveEdgeSnaps {
    /// Lines to draw showing active snaps (start, end, color)
    pub snap_lines: Vec<(Vec3, Vec3)>,
}

/// Tracks whether axis constraint was set via gizmo click (should clear on release)
#[derive(Debug, Default, Resource)]
pub struct GizmoAxisConstraint {
    /// True if current axis constraint was set by clicking the gizmo
    pub from_gizmo: bool,
}

/// Tracks the selected control point index within the currently selected spline
#[derive(Debug, Clone, Copy, Default, Resource)]
pub struct SelectedControlPointIndex(pub Option<usize>);

/// State for spline control point snap-to-object mode (T key while editing spline)
#[derive(Debug, Default, Resource)]
pub struct ControlPointSnapState {
    /// Whether snap mode is currently active
    pub active: bool,
    /// The spline entity being edited
    pub spline_entity: Option<Entity>,
    /// The control point index being snapped
    pub point_index: Option<usize>,
    /// The original local-space position of the control point (for cancel/undo)
    pub original_local_pos: Option<Vec3>,
}

impl ControlPointSnapState {
    pub fn reset(&mut self) {
        self.active = false;
        self.spline_entity = None;
        self.point_index = None;
        self.original_local_pos = None;
    }
}

/// Type of object being inserted in Insert mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertObjectType {
    Primitive(PrimitiveShape),
    PointLight,
    DirectionalLight,
    Group,
    /// GLTF/GLB model (path stored in InsertState.gltf_path)
    Gltf,
    /// RON scene file (path stored in InsertState.scene_path)
    Scene,
    /// Spline with the specified type
    Spline(SplineType),
    /// Volumetric fog volume
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

/// Marker component for preview entities in Insert mode
#[derive(Component)]
pub struct InsertPreview;

/// State for Insert mode
#[derive(Resource, Default)]
pub struct InsertState {
    /// The type of object being inserted (None if not inserting)
    pub object_type: Option<InsertObjectType>,
    /// The preview entity being positioned
    pub preview_entity: Option<Entity>,
    /// Default distance from camera when no surface is hit
    pub default_distance: f32,
    /// Path for GLTF objects (used when object_type is Gltf)
    pub gltf_path: Option<String>,
    /// Path for Scene objects (used when object_type is Scene)
    pub scene_path: Option<String>,
}

impl InsertState {
    pub fn new() -> Self {
        Self {
            object_type: None,
            preview_entity: None,
            default_distance: 10.0,
            gltf_path: None,
            scene_path: None,
        }
    }
}

/// Event to toggle physics debug rendering
#[derive(Message)]
pub struct TogglePhysicsDebugEvent;

/// Event to toggle physics simulation on/off
#[derive(Message)]
pub struct TogglePhysicsEvent;

/// Event to toggle the infinite grid visibility
#[derive(Message)]
pub struct ToggleGridEvent;

/// Event to toggle preview mode (hides all gizmos and debug rendering)
#[derive(Message)]
pub struct TogglePreviewModeEvent;

/// Event to toggle the editor on/off (F10)
/// When off, all UI and hotkeys are disabled
#[derive(Message)]
pub struct ToggleEditorEvent;

// ---------------------------------------------------------------------------
// Viewport Shading
// ---------------------------------------------------------------------------

/// Viewport shading mode for scene visualization
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Resource, Reflect)]
pub enum ViewportShadingMode {
    /// Normal rendering with full materials and lighting
    #[default]
    Rendered,
    /// Solid colors without textures (base color only, no textures)
    Solid,
    /// Wireframe overlay on top of solid shading
    Wireframe,
    /// Unlit rendering (materials without lighting calculations)
    Unlit,
}

impl ViewportShadingMode {
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Rendered => "Rendered",
            Self::Solid => "Solid",
            Self::Wireframe => "Wireframe",
            Self::Unlit => "Unlit",
        }
    }

    pub fn cycle_next(&self) -> Self {
        match self {
            Self::Rendered => Self::Solid,
            Self::Solid => Self::Wireframe,
            Self::Wireframe => Self::Unlit,
            Self::Unlit => Self::Rendered,
        }
    }
}

/// Event to set a specific viewport shading mode
#[derive(Message)]
pub struct SetShadingModeEvent(pub ViewportShadingMode);

/// Event to cycle to the next shading mode
#[derive(Message)]
pub struct CycleShadingModeEvent;

/// Event to start inserting an object in Insert mode
#[derive(Message)]
pub struct StartInsertEvent {
    pub object_type: InsertObjectType,
}

pub struct EditorStatePlugin;

impl Plugin for EditorStatePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<EditorMode>()
            // Simulation state and events (always registered so the editor
            // has the types; GamePlugin adds the actual systems)
            .init_state::<GameState>()
            .init_resource::<GameSnapshot>()
            .add_message::<PlayEvent>()
            .add_message::<PauseEvent>()
            .add_message::<ResetEvent>()
            // Lifecycle events (editor fires, games react)
            .add_message::<GameStartedEvent>()
            .add_message::<GameResumedEvent>()
            .add_message::<GamePausedEvent>()
            .add_message::<GameResetEvent>()
            .init_resource::<TransformOperation>()
            .init_resource::<AxisConstraint>()
            .init_resource::<SnapSubMode>()
            .init_resource::<EditorState>()
            .init_resource::<EditStepAmount>()
            .init_resource::<SelectedControlPointIndex>()
            .init_resource::<DimensionSnapSettings>()
            .init_resource::<ActiveEdgeSnaps>()
            .init_resource::<GizmoAxisConstraint>()
            .init_resource::<ControlPointSnapState>()
            .insert_resource(InsertState::new())
            .add_message::<TogglePhysicsDebugEvent>()
            .add_message::<TogglePhysicsEvent>()
            .add_message::<ToggleGridEvent>()
            .add_message::<TogglePreviewModeEvent>()
            .add_message::<ToggleEditorEvent>()
            .add_message::<StartInsertEvent>()
            .add_message::<SetShadingModeEvent>()
            .add_message::<CycleShadingModeEvent>()
            .init_resource::<ViewportShadingMode>()
            .add_systems(
                Update,
                (
                    handle_toggle_physics_debug,
                    handle_toggle_physics,
                    handle_toggle_grid,
                    handle_toggle_preview_mode,
                    handle_toggle_editor,
                    handle_set_shading_mode,
                    handle_cycle_shading_mode,
                ),
            )
            .add_systems(PostUpdate, keep_spatial_query_updated);
    }
}

/// Handle toggling physics debug rendering
fn handle_toggle_physics_debug(
    mut events: MessageReader<TogglePhysicsDebugEvent>,
    mut gizmo_config: ResMut<GizmoConfigStore>,
) {
    for _ in events.read() {
        let config = gizmo_config.config_mut::<PhysicsGizmos>().0;
        config.enabled = !config.enabled;
        info!("Physics debug: {}", if config.enabled { "ON" } else { "OFF" });
    }
}

/// Handle toggling physics simulation.
/// Ignored when GameState is not Editing to prevent conflicts with play/pause.
fn handle_toggle_physics(
    mut events: MessageReader<TogglePhysicsEvent>,
    mut physics_time: ResMut<Time<Physics>>,
    game_state: Res<State<GameState>>,
) {
    for _ in events.read() {
        if *game_state.get() != GameState::Editing {
            info!("Physics toggle ignored (game is not in Editing state)");
            continue;
        }
        if physics_time.relative_speed() == 0.0 {
            physics_time.set_relative_speed(1.0);
            info!("Physics simulation: RUNNING");
        } else {
            physics_time.set_relative_speed(0.0);
            info!("Physics simulation: PAUSED");
        }
    }
}

/// Handle toggling the infinite grid visibility
fn handle_toggle_grid(
    mut events: MessageReader<ToggleGridEvent>,
    mut grids: Query<&mut Visibility, With<InfiniteGridSettings>>,
) {
    for _ in events.read() {
        for mut visibility in grids.iter_mut() {
            *visibility = match *visibility {
                Visibility::Inherited | Visibility::Visible => Visibility::Hidden,
                Visibility::Hidden => Visibility::Visible,
            };
            info!(
                "Infinite grid: {}",
                if *visibility == Visibility::Hidden {
                    "HIDDEN"
                } else {
                    "VISIBLE"
                }
            );
        }
    }
}

/// Handle toggling preview mode (disables all gizmos and physics debug)
fn handle_toggle_preview_mode(
    mut events: MessageReader<TogglePreviewModeEvent>,
    mut editor_state: ResMut<EditorState>,
    mut gizmo_config: ResMut<GizmoConfigStore>,
    mut grids: Query<&mut Visibility, With<InfiniteGridSettings>>,
) {
    for _ in events.read() {
        // Toggle gizmos visibility
        editor_state.gizmos_visible = !editor_state.gizmos_visible;

        // Toggle physics debug gizmos
        let physics_config = gizmo_config.config_mut::<PhysicsGizmos>().0;
        physics_config.enabled = editor_state.gizmos_visible;

        // Toggle grid visibility
        for mut visibility in grids.iter_mut() {
            *visibility = if editor_state.gizmos_visible {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }

        info!(
            "Preview mode: {}",
            if editor_state.gizmos_visible { "OFF" } else { "ON" }
        );
    }
}

/// Handle toggling the editor on/off
fn handle_toggle_editor(
    mut events: MessageReader<ToggleEditorEvent>,
    mut editor_state: ResMut<EditorState>,
    mut gizmo_config: ResMut<GizmoConfigStore>,
    mut grids: Query<&mut Visibility, With<InfiniteGridSettings>>,
) {
    for _ in events.read() {
        editor_state.editor_active = !editor_state.editor_active;

        if editor_state.editor_active {
            // Re-enable UI and gizmos
            editor_state.ui_enabled = true;
            editor_state.gizmos_visible = true;

            // Enable physics debug gizmos
            let physics_config = gizmo_config.config_mut::<PhysicsGizmos>().0;
            physics_config.enabled = true;

            // Show grid
            for mut visibility in grids.iter_mut() {
                *visibility = Visibility::Visible;
            }
        } else {
            // Disable UI and gizmos
            editor_state.ui_enabled = false;
            editor_state.gizmos_visible = false;

            // Disable physics debug gizmos
            let physics_config = gizmo_config.config_mut::<PhysicsGizmos>().0;
            physics_config.enabled = false;

            // Hide grid
            for mut visibility in grids.iter_mut() {
                *visibility = Visibility::Hidden;
            }
        }

        info!(
            "Editor: {}",
            if editor_state.editor_active { "ON" } else { "OFF" }
        );
    }
}

/// Handle setting a specific shading mode
fn handle_set_shading_mode(
    mut events: MessageReader<SetShadingModeEvent>,
    mut shading_mode: ResMut<ViewportShadingMode>,
) {
    for event in events.read() {
        if *shading_mode != event.0 {
            *shading_mode = event.0;
            info!("Viewport shading: {}", shading_mode.display_name());
        }
    }
}

/// Handle cycling through shading modes
fn handle_cycle_shading_mode(
    mut events: MessageReader<CycleShadingModeEvent>,
    mut shading_mode: ResMut<ViewportShadingMode>,
) {
    for _ in events.read() {
        *shading_mode = shading_mode.cycle_next();
        info!("Viewport shading: {}", shading_mode.display_name());
    }
}

/// Keep the spatial query BVH in sync with Transform when physics is paused.
///
/// When physics is running, Avian3D handles Position/Rotation sync and BVH
/// rebuilds automatically. When paused, editor transforms still change but
/// Position/Rotation and the BVH go stale, breaking selection raycasts.
fn keep_spatial_query_updated(
    physics_time: Res<Time<Physics>>,
    mut colliders: Query<
        (Entity, &Transform, &mut Position, &mut Rotation, &Collider, &CollisionLayers),
        Without<ColliderDisabled>,
    >,
    mut pipeline: ResMut<SpatialQueryPipeline>,
) {
    if physics_time.relative_speed() != 0.0 {
        return;
    }

    // Sync Position/Rotation from Transform and collect BVH data in one pass
    let mut collider_data = Vec::new();
    for (entity, transform, mut position, mut rotation, collider, layers) in &mut colliders {
        let new_pos = Position::new(transform.translation);
        let new_rot = Rotation::from(transform.rotation);
        if *position != new_pos {
            *position = new_pos;
        }
        if *rotation != new_rot {
            *rotation = new_rot;
        }
        collider_data.push((entity, *position, *rotation, collider.clone(), *layers));
    }

    // Rebuild BVH
    pipeline.update(
        collider_data
            .iter()
            .map(|(e, p, r, c, l)| (*e, p, r, c, l)),
    );
}
