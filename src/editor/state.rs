use avian3d::debug_render::PhysicsGizmos;
use avian3d::prelude::Physics;
use avian3d::schedule::PhysicsTime;
use bevy::gizmos::config::GizmoConfigStore;
use bevy::prelude::*;
use bevy_infinite_grid::InfiniteGridSettings;

use crate::scene::PrimitiveShape;

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

/// Event to start inserting an object in Insert mode
#[derive(Message)]
pub struct StartInsertEvent {
    pub object_type: InsertObjectType,
}

pub struct EditorStatePlugin;

impl Plugin for EditorStatePlugin {
    fn build(&self, app: &mut App) {
        app.init_state::<EditorMode>()
            .init_resource::<TransformOperation>()
            .init_resource::<AxisConstraint>()
            .init_resource::<SnapSubMode>()
            .init_resource::<EditorState>()
            .init_resource::<EditStepAmount>()
            .insert_resource(InsertState::new())
            .add_message::<TogglePhysicsDebugEvent>()
            .add_message::<TogglePhysicsEvent>()
            .add_message::<ToggleGridEvent>()
            .add_message::<TogglePreviewModeEvent>()
            .add_message::<ToggleEditorEvent>()
            .add_message::<StartInsertEvent>()
            .add_systems(
                Update,
                (
                    handle_toggle_physics_debug,
                    handle_toggle_physics,
                    handle_toggle_grid,
                    handle_toggle_preview_mode,
                    handle_toggle_editor,
                ),
            );
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

/// Handle toggling physics simulation
fn handle_toggle_physics(
    mut events: MessageReader<TogglePhysicsEvent>,
    mut physics_time: ResMut<Time<Physics>>,
) {
    for _ in events.read() {
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
