use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::any::TypeId;

use bevy_spline_3d::distribution::{DistributionOrientation, DistributionSource, SplineDistribution};
use bevy_spline_3d::prelude::{Spline, SplineType};

use crate::commands::{RedoEvent, TakeSnapshotCommand, UndoEvent};
use crate::editor::{
    CameraMarks, EditorMode, EditorState, InsertObjectType, JumpToLastPositionEvent,
    JumpToMarkEvent, SetCameraMarkEvent, StartInsertEvent, ToggleGridEvent, TogglePhysicsDebugEvent,
    TogglePhysicsEvent,
};
use crate::scene::{
    PrimitiveShape, SceneFile, SpawnDemoSceneEvent, SpawnEntityEvent, SpawnEntityKind,
    UnparentSelectedEvent,
};
use crate::selection::Selected;
use crate::ui::component_browser::{add_component_by_type_id, ComponentRegistry};
use crate::ui::file_dialog::FileDialogState;
use crate::ui::theme::colors;
use crate::ui::SettingsWindowState;
use crate::utils::should_process_input;

/// System parameter grouping all command palette event writers
#[derive(SystemParam)]
struct CommandEvents<'w> {
    spawn_entity: MessageWriter<'w, SpawnEntityEvent>,
    unparent: MessageWriter<'w, UnparentSelectedEvent>,
    set_mark: MessageWriter<'w, SetCameraMarkEvent>,
    jump_mark: MessageWriter<'w, JumpToMarkEvent>,
    jump_last: MessageWriter<'w, JumpToLastPositionEvent>,
    toggle_debug: MessageWriter<'w, TogglePhysicsDebugEvent>,
    toggle_physics: MessageWriter<'w, TogglePhysicsEvent>,
    toggle_grid: MessageWriter<'w, ToggleGridEvent>,
    start_insert: MessageWriter<'w, StartInsertEvent>,
    spawn_demo: MessageWriter<'w, SpawnDemoSceneEvent>,
    undo: MessageWriter<'w, UndoEvent>,
    redo: MessageWriter<'w, RedoEvent>,
}

/// System parameter grouping palette UI state resources
#[derive(SystemParam)]
struct PaletteState2<'w> {
    help_state: ResMut<'w, HelpWindowState>,
    settings_state: ResMut<'w, SettingsWindowState>,
    custom_mark_state: ResMut<'w, CustomMarkDialogState>,
    component_editor_state: ResMut<'w, super::inspector::ComponentEditorState>,
    component_registry: ResMut<'w, ComponentRegistry>,
    removable_cache: Res<'w, RemovableComponentsCache>,
}

/// A command that can be executed from the palette
#[derive(Clone)]
pub struct Command {
    /// Display name
    pub name: String,
    /// Search keywords (includes name)
    pub keywords: Vec<String>,
    /// Category for grouping
    pub category: &'static str,
    /// The action to perform
    pub action: CommandAction,
    /// Whether this command is available in Insert mode (creates objects)
    pub insertable: bool,
}

/// Actions that commands can perform
#[derive(Clone)]
pub enum CommandAction {
    SpawnPrimitive(PrimitiveShape),
    SpawnPointLight,
    SpawnDirectionalLight,
    SetCameraMark(String),
    JumpToMark(String),
    JumpToLastPosition,
    SaveScene,
    LoadScene,
    ShowHelp,
    OpenSettings,
    SetGridSnap(f32),
    SetRotationSnap(f32),
    ShowCustomMarkDialog,
    SpawnGroup,
    UnparentSelected,
    TogglePhysicsDebug,
    TogglePhysics,
    ToggleGrid,
    SpawnDemoScene,
    Undo,
    Redo,
    AddComponent,
    /// Open file dialog to insert a GLTF/GLB model
    InsertGltf,
    /// Open file dialog to insert a RON scene file
    InsertScene,
    /// Spawn a spline of the specified type
    SpawnSpline(SplineType),
    /// Spawn a volumetric fog volume
    SpawnFogVolume,
    /// Create a distribution from selected entities (requires 1 spline + 1 source selected)
    CreateDistribution,
}

/// The mode the command palette is operating in
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    /// Normal command search
    #[default]
    Commands,
    /// Insert mode - only show insertable objects
    Insert,
    /// Component search - show components on selected entity
    ComponentSearch,
    /// Add component - show all available components to add
    AddComponent,
    /// Remove component - show components that can be removed
    RemoveComponent,
}

/// Resource to track command palette state
#[derive(Resource)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected_index: usize,
    /// Whether we just opened (to focus the input)
    pub just_opened: bool,
    /// The current palette mode
    pub mode: PaletteMode,
    /// Target entity for AddComponent mode
    pub target_entity: Option<Entity>,
}

/// Resource to track help window state
#[derive(Resource, Default)]
pub struct HelpWindowState {
    pub open: bool,
}

/// Resource to track custom mark name dialog state
#[derive(Resource, Default)]
pub struct CustomMarkDialogState {
    pub open: bool,
    pub name: String,
    pub just_opened: bool,
}

/// Cached list of removable components for an entity
#[derive(Resource, Default)]
pub struct RemovableComponentsCache {
    pub entity: Option<Entity>,
    pub components: Vec<(TypeId, String)>,
}

impl Default for CommandPaletteState {
    fn default() -> Self {
        Self {
            open: false,
            query: String::new(),
            selected_index: 0,
            just_opened: false,
            mode: PaletteMode::Commands,
            target_entity: None,
        }
    }
}

impl CommandPaletteState {
    /// Reset and open the palette in a specific mode
    fn open_mode(&mut self, mode: PaletteMode) {
        self.open = true;
        self.query.clear();
        self.selected_index = 0;
        self.just_opened = true;
        self.mode = mode;
        self.target_entity = None;
    }

    /// Open the palette in Commands mode
    pub fn open_commands(&mut self) {
        self.open_mode(PaletteMode::Commands);
    }

    /// Open the palette in Insert mode
    pub fn open_insert(&mut self) {
        self.open_mode(PaletteMode::Insert);
    }

    /// Open the palette in ComponentSearch mode
    pub fn open_component_search(&mut self) {
        self.open_mode(PaletteMode::ComponentSearch);
    }

    /// Open the palette in AddComponent mode for a specific entity
    pub fn open_add_component(&mut self, entity: Entity) {
        self.open_mode(PaletteMode::AddComponent);
        self.target_entity = Some(entity);
    }

    /// Open the palette in RemoveComponent mode for a specific entity
    pub fn open_remove_component(&mut self, entity: Entity) {
        self.open_mode(PaletteMode::RemoveComponent);
        self.target_entity = Some(entity);
    }
}

/// Open the command palette in AddComponent mode for a specific entity
pub fn open_add_component_palette(state: &mut CommandPaletteState, entity: Entity) {
    state.open_add_component(entity);
}

/// Resource containing all available commands
#[derive(Resource, Default)]
pub struct CommandRegistry {
    pub commands: Vec<Command>,
}

impl CommandRegistry {
    /// Build the static commands list
    pub fn build_static_commands(&mut self) {
        self.commands.clear();

        // Primitive spawning (insertable)
        self.commands.push(Command {
            name: "Add Cube".to_string(),
            keywords: vec!["box".into(), "primitive".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Cube),
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add Sphere".to_string(),
            keywords: vec!["ball".into(), "primitive".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Sphere),
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add Cylinder".to_string(),
            keywords: vec!["tube".into(), "primitive".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Cylinder),
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add Capsule".to_string(),
            keywords: vec!["pill".into(), "primitive".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Capsule),
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add Plane".to_string(),
            keywords: vec!["floor".into(), "ground".into(), "primitive".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Plane),
            insertable: true,
        });

        // Lights (insertable)
        self.commands.push(Command {
            name: "Add Point Light".to_string(),
            keywords: vec!["lamp".into(), "bulb".into(), "lighting".into()],
            category: "Lights",
            action: CommandAction::SpawnPointLight,
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add Sun Light".to_string(),
            keywords: vec!["directional".into(), "sun".into(), "lighting".into(), "shadow".into()],
            category: "Lights",
            action: CommandAction::SpawnDirectionalLight,
            insertable: true,
        });

        // Models
        self.commands.push(Command {
            name: "Add GLTF Model".to_string(),
            keywords: vec!["model".into(), "glb".into(), "mesh".into(), "import".into(), "3d".into()],
            category: "Models",
            action: CommandAction::InsertGltf,
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add Scene".to_string(),
            keywords: vec!["import".into(), "ron".into(), "nested".into(), "sub".into()],
            category: "Models",
            action: CommandAction::InsertScene,
            insertable: true,
        });

        // Splines (insertable)
        self.commands.push(Command {
            name: "Add Bezier Spline".to_string(),
            keywords: vec!["curve".into(), "path".into(), "cubic".into(), "bezier".into()],
            category: "Splines",
            action: CommandAction::SpawnSpline(SplineType::CubicBezier),
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add Catmull-Rom Spline".to_string(),
            keywords: vec!["curve".into(), "path".into(), "catmull".into(), "rom".into()],
            category: "Splines",
            action: CommandAction::SpawnSpline(SplineType::CatmullRom),
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add B-Spline".to_string(),
            keywords: vec!["curve".into(), "path".into(), "bspline".into()],
            category: "Splines",
            action: CommandAction::SpawnSpline(SplineType::BSpline),
            insertable: true,
        });
        self.commands.push(Command {
            name: "Create Distribution".to_string(),
            keywords: vec!["distribute".into(), "clone".into(), "array".into(), "spline".into(), "copy".into(), "instances".into()],
            category: "Splines",
            action: CommandAction::CreateDistribution,
            insertable: false,
        });

        // Effects (insertable)
        self.commands.push(Command {
            name: "Add Fog Volume".to_string(),
            keywords: vec!["volumetric".into(), "fog".into(), "atmosphere".into(), "mist".into(), "haze".into()],
            category: "Effects",
            action: CommandAction::SpawnFogVolume,
            insertable: true,
        });

        // Scene operations
        self.commands.push(Command {
            name: "Save Scene".to_string(),
            keywords: vec!["export".into(), "file".into()],
            category: "Scene",
            action: CommandAction::SaveScene,
            insertable: false,
        });
        self.commands.push(Command {
            name: "Load Scene".to_string(),
            keywords: vec!["import".into(), "open".into(), "file".into()],
            category: "Scene",
            action: CommandAction::LoadScene,
            insertable: false,
        });
        self.commands.push(Command {
            name: "Spawn Demo Scene".to_string(),
            keywords: vec!["example".into(), "sample".into(), "test".into(), "create".into()],
            category: "Scene",
            action: CommandAction::SpawnDemoScene,
            insertable: false,
        });

        // Groups (insertable)
        self.commands.push(Command {
            name: "Add Group".to_string(),
            keywords: vec!["folder".into(), "container".into(), "nest".into()],
            category: "Primitives",
            action: CommandAction::SpawnGroup,
            insertable: true,
        });
        self.commands.push(Command {
            name: "Unparent Selected".to_string(),
            keywords: vec!["detach".into(), "remove".into(), "parent".into()],
            category: "Hierarchy",
            action: CommandAction::UnparentSelected,
            insertable: false,
        });

        // Camera marks
        self.commands.push(Command {
            name: "Jump to Last Position".to_string(),
            keywords: vec!["back".into(), "previous".into(), "camera".into()],
            category: "Camera",
            action: CommandAction::JumpToLastPosition,
            insertable: false,
        });
        self.commands.push(Command {
            name: "Set Custom Camera Mark".to_string(),
            keywords: vec!["save".into(), "bookmark".into(), "name".into(), "camera".into()],
            category: "Camera",
            action: CommandAction::ShowCustomMarkDialog,
            insertable: false,
        });

        // Help
        self.commands.push(Command {
            name: "Help: Keyboard Shortcuts".to_string(),
            keywords: vec!["hotkeys".into(), "keys".into(), "bindings".into(), "controls".into()],
            category: "Help",
            action: CommandAction::ShowHelp,
            insertable: false,
        });

        // Settings
        self.commands.push(Command {
            name: "Settings".to_string(),
            keywords: vec!["preferences".into(), "options".into(), "config".into(), "configuration".into()],
            category: "Settings",
            action: CommandAction::OpenSettings,
            insertable: false,
        });

        // Edit operations
        self.commands.push(Command {
            name: "Undo".to_string(),
            keywords: vec!["back".into(), "revert".into(), "history".into()],
            category: "Edit",
            action: CommandAction::Undo,
            insertable: false,
        });
        self.commands.push(Command {
            name: "Redo".to_string(),
            keywords: vec!["forward".into(), "repeat".into(), "history".into()],
            category: "Edit",
            action: CommandAction::Redo,
            insertable: false,
        });
        self.commands.push(Command {
            name: "Add Component".to_string(),
            keywords: vec!["component".into(), "attach".into(), "insert".into(), "reflection".into()],
            category: "Edit",
            action: CommandAction::AddComponent,
            insertable: false,
        });

        // Debug / View
        self.commands.push(Command {
            name: "Toggle Physics Debug".to_string(),
            keywords: vec!["collider".into(), "collision".into(), "gizmo".into(), "wireframe".into(), "avian".into()],
            category: "Debug",
            action: CommandAction::TogglePhysicsDebug,
            insertable: false,
        });
        self.commands.push(Command {
            name: "Toggle Physics Simulation".to_string(),
            keywords: vec!["pause".into(), "play".into(), "freeze".into(), "stop".into(), "run".into()],
            category: "Physics",
            action: CommandAction::TogglePhysics,
            insertable: false,
        });
        self.commands.push(Command {
            name: "Toggle Grid".to_string(),
            keywords: vec!["floor".into(), "infinite".into(), "hide".into(), "show".into(), "visible".into()],
            category: "View",
            action: CommandAction::ToggleGrid,
            insertable: false,
        });

        // Grid snap
        self.commands.push(Command {
            name: "Grid Snap: Off".to_string(),
            keywords: vec!["disable".into(), "none".into()],
            category: "Snapping",
            action: CommandAction::SetGridSnap(0.0),
            insertable: false,
        });
        self.commands.push(Command {
            name: "Grid Snap: 0.25".to_string(),
            keywords: vec!["quarter".into()],
            category: "Snapping",
            action: CommandAction::SetGridSnap(0.25),
            insertable: false,
        });
        self.commands.push(Command {
            name: "Grid Snap: 0.5".to_string(),
            keywords: vec!["half".into()],
            category: "Snapping",
            action: CommandAction::SetGridSnap(0.5),
            insertable: false,
        });
        self.commands.push(Command {
            name: "Grid Snap: 1.0".to_string(),
            keywords: vec!["one".into(), "unit".into()],
            category: "Snapping",
            action: CommandAction::SetGridSnap(1.0),
            insertable: false,
        });
        self.commands.push(Command {
            name: "Grid Snap: 2.0".to_string(),
            keywords: vec!["two".into()],
            category: "Snapping",
            action: CommandAction::SetGridSnap(2.0),
            insertable: false,
        });

        // Rotation snap
        self.commands.push(Command {
            name: "Rotation Snap: Off".to_string(),
            keywords: vec!["angle".into(), "disable".into(), "none".into()],
            category: "Snapping",
            action: CommandAction::SetRotationSnap(0.0),
            insertable: false,
        });
        self.commands.push(Command {
            name: "Rotation Snap: 15°".to_string(),
            keywords: vec!["angle".into(), "degrees".into()],
            category: "Snapping",
            action: CommandAction::SetRotationSnap(15.0),
            insertable: false,
        });
        self.commands.push(Command {
            name: "Rotation Snap: 45°".to_string(),
            keywords: vec!["angle".into(), "degrees".into()],
            category: "Snapping",
            action: CommandAction::SetRotationSnap(45.0),
            insertable: false,
        });
        self.commands.push(Command {
            name: "Rotation Snap: 90°".to_string(),
            keywords: vec!["angle".into(), "degrees".into(), "right".into()],
            category: "Snapping",
            action: CommandAction::SetRotationSnap(90.0),
            insertable: false,
        });
    }

    /// Add dynamic commands based on current state (like existing marks)
    pub fn add_mark_commands(&mut self, marks: &CameraMarks) {
        // Remove old mark commands
        self.commands.retain(|cmd| {
            !matches!(cmd.action, CommandAction::JumpToMark(_) | CommandAction::SetCameraMark(_))
        });

        // Add jump commands for existing marks
        for name in marks.marks.keys() {
            self.commands.push(Command {
                name: format!("Jump to Mark: {}", name),
                keywords: vec!["goto".into(), "camera".into()],
                category: "Camera Marks",
                action: CommandAction::JumpToMark(name.clone()),
                insertable: false,
            });
        }

        // Add set mark commands for quick marks 1-9
        for i in 1..=9 {
            self.commands.push(Command {
                name: format!("Set Mark {}", i),
                keywords: vec!["save".into(), "camera".into()],
                category: "Camera Marks",
                action: CommandAction::SetCameraMark(i.to_string()),
                insertable: false,
            });
        }
    }
}

/// Get filtered and sorted commands based on query using skim fuzzy matcher
fn filter_commands<'a>(
    commands: &'a [Command],
    query: &str,
    insert_mode: bool,
) -> Vec<(usize, &'a Command, i64)> {
    let matcher = SkimMatcherV2::default();

    // First filter by insert mode if applicable
    let mode_filtered: Vec<_> = commands
        .iter()
        .enumerate()
        .filter(|(_, cmd)| !insert_mode || cmd.insertable)
        .collect();

    if query.is_empty() {
        // Return all commands with score 0 when no query
        return mode_filtered
            .into_iter()
            .map(|(idx, cmd)| (idx, cmd, 0i64))
            .collect();
    }

    let mut results: Vec<(usize, &Command, i64)> = mode_filtered
        .into_iter()
        .filter_map(|(idx, cmd)| {
            // Check name first
            if let Some(score) = matcher.fuzzy_match(&cmd.name, query) {
                return Some((idx, cmd, score));
            }

            // Check keywords - find best match
            let best_keyword_score = cmd
                .keywords
                .iter()
                .filter_map(|kw| matcher.fuzzy_match(kw, query))
                .max();

            if let Some(score) = best_keyword_score {
                // Significant penalty for keyword-only match (name matches should rank higher)
                return Some((idx, cmd, score / 2));
            }

            None
        })
        .collect();

    // Sort by score (higher is better with skim)
    results.sort_by(|a, b| b.2.cmp(&a.2));
    results
}

pub struct CommandPalettePlugin;

impl Plugin for CommandPalettePlugin {
    fn build(&self, app: &mut App) {
        let mut registry = CommandRegistry::default();
        registry.build_static_commands();

        app.init_resource::<CommandPaletteState>()
            .init_resource::<HelpWindowState>()
            .init_resource::<CustomMarkDialogState>()
            .init_resource::<RemovableComponentsCache>()
            .insert_resource(registry)
            .add_systems(Update, (handle_palette_toggle, populate_removable_components))
            .add_systems(EguiPrimaryContextPass, (draw_command_palette, draw_help_window, draw_custom_mark_dialog));
    }
}

/// Populate the removable components cache when RemoveComponent mode is active
fn populate_removable_components(world: &mut World) {
    // Check if we're in RemoveComponent mode
    let state = world.resource::<CommandPaletteState>();
    if state.mode != PaletteMode::RemoveComponent || !state.open {
        return;
    }

    let target_entity = state.target_entity;
    let Some(entity) = target_entity else {
        return;
    };

    // Check if cache is already populated for this entity
    let cache = world.resource::<RemovableComponentsCache>();
    if cache.entity == Some(entity) && !cache.components.is_empty() {
        return;
    }

    // Get the type registry
    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry_guard = type_registry.read();

    // Get components on this entity
    let entity_ref = world.entity(entity);
    let archetype = entity_ref.archetype();

    let mut components: Vec<(TypeId, String)> = archetype
        .components()
        .iter()
        .filter_map(|component_id| {
            let component_info = world.components().get_info(*component_id)?;
            let type_id = component_info.type_id()?;

            // Check if this type is registered for reflection
            let registration = type_registry_guard.get(type_id)?;

            // Check if it has ReflectComponent (can be removed)
            registration.data::<ReflectComponent>()?;

            let short_name = registration
                .type_info()
                .type_path_table()
                .short_path()
                .to_string();

            // Skip core components that shouldn't be removed
            if short_name == "Transform"
                || short_name == "GlobalTransform"
                || short_name == "Visibility"
                || short_name == "InheritedVisibility"
                || short_name == "ViewVisibility"
                || short_name == "SceneEntity"
            {
                return None;
            }

            Some((type_id, short_name))
        })
        .collect();

    components.sort_by(|a, b| a.1.cmp(&b.1));

    drop(type_registry_guard);

    // Update the cache
    let mut cache = world.resource_mut::<RemovableComponentsCache>();
    cache.entity = Some(entity);
    cache.components = components;
}

/// Open palette with C key, or / key for component search in ObjectInspector mode
/// Also handles ? (Shift+/) to open help window and X to remove component
fn handle_palette_toggle(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<CommandPaletteState>,
    mut help_state: ResMut<HelpWindowState>,
    mut registry: ResMut<CommandRegistry>,
    mut removable_cache: ResMut<RemovableComponentsCache>,
    marks: Res<CameraMarks>,
    editor_mode: Res<State<EditorMode>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
    selected: Query<Entity, With<Selected>>,
) {
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }

    // "?" (Shift+/) opens help window
    let shift = keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    if keyboard.just_pressed(KeyCode::Slash) && shift {
        help_state.open = !help_state.open;
        return;
    }

    // Don't open palette if already open
    if state.open {
        return;
    }

    // "/" key opens component search in ObjectInspector mode
    if keyboard.just_pressed(KeyCode::Slash) && *editor_mode.get() == EditorMode::ObjectInspector {
        state.open_component_search();
        return;
    }

    // "X" key opens remove component palette in ObjectInspector mode
    if keyboard.just_pressed(KeyCode::KeyX) && *editor_mode.get() == EditorMode::ObjectInspector {
        if let Some(entity) = selected.iter().next() {
            // Clear cache to force refresh
            removable_cache.entity = None;
            removable_cache.components.clear();
            state.open_remove_component(entity);
        }
        return;
    }

    if keyboard.just_pressed(KeyCode::KeyC) {
        state.open_commands();
        // Refresh dynamic commands
        registry.add_mark_commands(&marks);
    }
}

/// Draw the command palette
fn draw_command_palette(
    mut contexts: EguiContexts,
    mut state: ResMut<CommandPaletteState>,
    mut palette_state2: PaletteState2,
    mut editor_state: ResMut<EditorState>,
    mut file_dialog_state: ResMut<FileDialogState>,
    scene_file: Res<SceneFile>,
    registry: Res<CommandRegistry>,
    type_registry: Res<AppTypeRegistry>,
    selected: Query<Entity, With<Selected>>,
    mut events: CommandEvents,
    mut commands: Commands,
    mut next_mode: ResMut<NextState<EditorMode>>,
) -> Result {
    // Don't draw UI when editor is disabled
    if !editor_state.ui_enabled {
        return Ok(());
    }

    if !state.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    // Handle ComponentSearch mode separately
    if state.mode == PaletteMode::ComponentSearch {
        return draw_component_search_palette(
            ctx,
            &mut state,
            &mut palette_state2.component_editor_state,
            &type_registry,
            &selected,
        );
    }

    // Handle AddComponent mode separately
    if state.mode == PaletteMode::AddComponent {
        return draw_add_component_palette(
            ctx,
            &mut state,
            &mut palette_state2.component_registry,
            &type_registry,
            &mut commands,
        );
    }

    // Handle RemoveComponent mode separately
    if state.mode == PaletteMode::RemoveComponent {
        return draw_remove_component_palette(
            ctx,
            &mut state,
            &palette_state2.removable_cache,
            &selected,
            &mut commands,
        );
    }

    let in_insert_mode = state.mode == PaletteMode::Insert;
    let filtered = filter_commands(&registry.commands, &state.query, in_insert_mode);

    // Clamp selected index
    if !filtered.is_empty() {
        state.selected_index = state.selected_index.min(filtered.len() - 1);
    }

    let mut should_close = false;
    let mut action_to_execute: Option<CommandAction> = None;

    // Check for keyboard input before rendering UI
    let enter_pressed = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let down_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
    let up_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));

    // Handle Enter to execute command
    if enter_pressed {
        if let Some((_, cmd, _)) = filtered.get(state.selected_index) {
            action_to_execute = Some(cmd.action.clone());
            should_close = true;
        }
    }

    // Handle Escape to close
    if escape_pressed {
        should_close = true;
    }

    // Handle arrow keys for navigation
    if down_pressed && !filtered.is_empty() {
        state.selected_index = (state.selected_index + 1).min(filtered.len() - 1);
    }
    if up_pressed {
        state.selected_index = state.selected_index.saturating_sub(1);
    }

    let title = if in_insert_mode {
        "Insert Object"
    } else {
        "Command Palette"
    };

    let hint = if in_insert_mode {
        "Type to search objects to insert..."
    } else {
        "Type to search commands..."
    };

    egui::Window::new(title)
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([400.0, 300.0])
        .show(ctx, |ui| {
            // Mode indicator for Insert mode
            if in_insert_mode {
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new("INSERT MODE")
                            .small()
                            .strong()
                            .color(colors::ACCENT_GREEN),
                    );
                    ui.label(
                        egui::RichText::new("- Select object, then click to place")
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                });
                ui.add_space(4.0);
            }

            // Search input
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text(hint)
                    .desired_width(f32::INFINITY),
            );

            // Focus the input when just opened
            if state.just_opened {
                response.request_focus();
                state.just_opened = false;
            }

            ui.separator();

            // Command list
            egui::ScrollArea::vertical()
                .max_height(250.0)
                .show(ui, |ui| {
                    if filtered.is_empty() {
                        ui.label(egui::RichText::new("No matching commands").color(colors::TEXT_MUTED));
                    } else {
                        let mut current_category: Option<&str> = None;

                        for (display_idx, (_, cmd, _)) in filtered.iter().enumerate() {
                            // Category header
                            if current_category != Some(cmd.category) {
                                current_category = Some(cmd.category);
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(cmd.category).small().color(colors::TEXT_MUTED));
                            }

                            let is_selected = display_idx == state.selected_index;
                            let text_color = if is_selected {
                                colors::TEXT_PRIMARY
                            } else {
                                colors::TEXT_SECONDARY
                            };

                            let response = ui.selectable_label(
                                is_selected,
                                egui::RichText::new(&cmd.name).color(text_color),
                            );

                            if response.clicked() {
                                action_to_execute = Some(cmd.action.clone());
                                should_close = true;
                            }

                            // Scroll selected item into view
                            if is_selected {
                                response.scroll_to_me(Some(egui::Align::Center));
                            }
                        }
                    }
                });

            ui.separator();

            // Help text
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Enter").small().strong().color(colors::ACCENT_BLUE));
                ui.label(egui::RichText::new("to select").small().color(colors::TEXT_MUTED));
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Esc").small().strong().color(colors::ACCENT_BLUE));
                ui.label(egui::RichText::new("to close").small().color(colors::TEXT_MUTED));
            });
        });

    // Execute action after UI
    if let Some(action) = action_to_execute {
        // In Insert mode, send event to create preview entity
        if in_insert_mode {
            match &action {
                CommandAction::SpawnPrimitive(shape) => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Primitive(*shape),
                    });
                }
                CommandAction::SpawnPointLight => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::PointLight,
                    });
                }
                CommandAction::SpawnDirectionalLight => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::DirectionalLight,
                    });
                }
                CommandAction::SpawnGroup => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Group,
                    });
                }
                CommandAction::InsertGltf => {
                    // Use same code path as command palette - exit insert mode first,
                    // file dialog will re-enter insert mode after file is picked
                    file_dialog_state.open_insert_gltf();
                    // Exit insert mode so the state transition triggers properly when file is picked
                    next_mode.set(EditorMode::View);
                }
                CommandAction::InsertScene => {
                    // Use same code path as command palette - exit insert mode first,
                    // file dialog will re-enter insert mode after file is picked
                    file_dialog_state.open_insert_scene();
                    // Exit insert mode so the state transition triggers properly when file is picked
                    next_mode.set(EditorMode::View);
                }
                CommandAction::SpawnSpline(spline_type) => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Spline(*spline_type),
                    });
                }
                CommandAction::SpawnFogVolume => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::FogVolume,
                    });
                }
                _ => {}
            }
        } else {
            // Normal mode - execute action immediately
            match action {
                CommandAction::SpawnPrimitive(shape) => {
                    events.spawn_entity.write(SpawnEntityEvent {
                        kind: SpawnEntityKind::Primitive(shape),
                        position: Vec3::ZERO,
                        rotation: Quat::IDENTITY,
                    });
                }
                CommandAction::SpawnPointLight => {
                    events.spawn_entity.write(SpawnEntityEvent {
                        kind: SpawnEntityKind::PointLight,
                        position: Vec3::new(0.0, 3.0, 0.0),
                        rotation: Quat::IDENTITY,
                    });
                }
                CommandAction::SpawnDirectionalLight => {
                    events.spawn_entity.write(SpawnEntityEvent {
                        kind: SpawnEntityKind::DirectionalLight,
                        position: Vec3::new(4.0, 8.0, 4.0),
                        rotation: Quat::IDENTITY,
                    });
                }
                CommandAction::SpawnSpline(spline_type) => {
                    events.spawn_entity.write(SpawnEntityEvent {
                        kind: SpawnEntityKind::Spline(spline_type),
                        position: Vec3::ZERO,
                        rotation: Quat::IDENTITY,
                    });
                }
                CommandAction::SpawnFogVolume => {
                    events.spawn_entity.write(SpawnEntityEvent {
                        kind: SpawnEntityKind::FogVolume,
                        position: Vec3::ZERO,
                        rotation: Quat::IDENTITY,
                    });
                }
                CommandAction::SetCameraMark(name) => {
                    events.set_mark.write(SetCameraMarkEvent { name });
                }
                CommandAction::JumpToMark(name) => {
                    events.jump_mark.write(JumpToMarkEvent { name });
                }
                CommandAction::JumpToLastPosition => {
                    events.jump_last.write(JumpToLastPositionEvent);
                }
                CommandAction::SaveScene => {
                    // Open the egui file dialog for saving
                    file_dialog_state.open_save_scene(scene_file.path.as_deref());
                }
                CommandAction::LoadScene => {
                    // Open the egui file dialog for loading
                    file_dialog_state.open_load_scene(scene_file.path.as_deref());
                }
                CommandAction::ShowHelp => {
                    palette_state2.help_state.open = true;
                }
                CommandAction::OpenSettings => {
                    palette_state2.settings_state.open = true;
                }
                CommandAction::SetGridSnap(value) => {
                    editor_state.grid_snap = value;
                }
                CommandAction::SetRotationSnap(value) => {
                    editor_state.rotation_snap = value;
                }
                CommandAction::ShowCustomMarkDialog => {
                    palette_state2.custom_mark_state.open = true;
                    palette_state2.custom_mark_state.name.clear();
                    palette_state2.custom_mark_state.just_opened = true;
                }
                CommandAction::SpawnGroup => {
                    events.spawn_entity.write(SpawnEntityEvent {
                        kind: SpawnEntityKind::Group,
                        position: Vec3::ZERO,
                        rotation: Quat::IDENTITY,
                    });
                }
                CommandAction::UnparentSelected => {
                    events.unparent.write(UnparentSelectedEvent);
                }
                CommandAction::TogglePhysicsDebug => {
                    events.toggle_debug.write(TogglePhysicsDebugEvent);
                }
                CommandAction::TogglePhysics => {
                    events.toggle_physics.write(TogglePhysicsEvent);
                }
                CommandAction::ToggleGrid => {
                    events.toggle_grid.write(ToggleGridEvent);
                }
                CommandAction::SpawnDemoScene => {
                    events.spawn_demo.write(SpawnDemoSceneEvent);
                }
                CommandAction::Undo => {
                    events.undo.write(UndoEvent);
                }
                CommandAction::Redo => {
                    events.redo.write(RedoEvent);
                }
                CommandAction::AddComponent => {
                    // Get first selected entity and switch to AddComponent mode
                    if let Some(entity) = selected.iter().next() {
                        state.open_add_component(entity);
                        // Don't close the palette, we're switching modes
                        should_close = false;
                    }
                }
                CommandAction::InsertGltf => {
                    // Open file dialog to pick a GLTF file
                    file_dialog_state.open_insert_gltf();
                }
                CommandAction::InsertScene => {
                    // Open file dialog to pick a RON scene file
                    file_dialog_state.open_insert_scene();
                }
                CommandAction::CreateDistribution => {
                    // Queue a deferred command to create the distribution
                    let selected_entities: Vec<Entity> = selected.iter().collect();
                    commands.queue(CreateDistributionCommand { selected_entities });
                }
            }
        }
    }

    if should_close {
        state.open = false;
    }

    Ok(())
}

/// Draw the help window with keyboard shortcuts
fn draw_help_window(
    mut contexts: EguiContexts,
    mut state: ResMut<HelpWindowState>,
    editor_state: Res<EditorState>,
) -> Result {
    // Don't draw UI when editor is disabled
    if !editor_state.ui_enabled {
        return Ok(());
    }

    if !state.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let mut should_close = false;

    egui::Window::new("Keyboard Shortcuts")
        .collapsible(false)
        .resizable(false)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Column 1: Modes & Camera
                ui.vertical(|ui| {
                    ui.set_min_width(220.0);

                    help_section(ui, "Mode Switching");
                    shortcut_row(ui, "V", "Toggle View/Edit mode");
                    shortcut_row(ui, "I", "Insert mode");
                    shortcut_row(ui, "O", "Object Inspector mode");
                    shortcut_row(ui, "H", "Hierarchy mode");
                    shortcut_row(ui, "Shift+key", "Switch from any mode");
                    shortcut_row(ui, "Esc", "Return to View mode");

                    ui.add_space(12.0);
                    help_section(ui, "Camera (View Mode)");
                    shortcut_row(ui, "W/A/S/D", "Move camera");
                    shortcut_row(ui, "Space/Ctrl", "Up/down (relative)");
                    shortcut_row(ui, "Shift", "Move faster");
                    shortcut_row(ui, "Right Mouse", "Look around");
                    shortcut_row(ui, "L", "Look at selected");
                    shortcut_row(ui, "1-9", "Jump to mark");
                    shortcut_row(ui, "Shift+1-9", "Set mark");
                    shortcut_row(ui, "`", "Last position");

                    ui.add_space(12.0);
                    help_section(ui, "Selection");
                    shortcut_row(ui, "Click", "Select object");
                    shortcut_row(ui, "Shift+Click", "Multi-select");
                    shortcut_row(ui, "G", "Group selected");
                    shortcut_row(ui, "Delete", "Delete selected");
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(16.0);

                // Column 2: Edit Mode & Commands
                ui.vertical(|ui| {
                    ui.set_min_width(220.0);

                    help_section(ui, "Edit Mode - Transform");
                    shortcut_row(ui, "Q", "Translate");
                    shortcut_row(ui, "W", "Rotate");
                    shortcut_row(ui, "E", "Scale");
                    shortcut_row(ui, "R", "Place (raycast)");
                    shortcut_row(ui, "T", "Snap to object");
                    shortcut_row(ui, "  A", "  Surface align");
                    shortcut_row(ui, "  S", "  Center align");
                    shortcut_row(ui, "  D", "  Aligned (rotated)");
                    shortcut_row(ui, "A/S/D", "Constrain X/Y/Z");
                    shortcut_row(ui, "J/K", "Step -/+");

                    ui.add_space(12.0);
                    help_section(ui, "Insert Mode (I)");
                    shortcut_row(ui, "Type", "Search objects");
                    shortcut_row(ui, "Enter", "Select type");
                    shortcut_row(ui, "Click", "Place object");
                    shortcut_row(ui, "Shift+Click", "Place multiple");
                    shortcut_row(ui, "Esc", "Cancel");

                    ui.add_space(12.0);
                    help_section(ui, "Commands");
                    shortcut_row(ui, "C", "Command palette");
                    shortcut_row(ui, "F", "Find object");
                    shortcut_row(ui, "U", "Undo");
                    shortcut_row(ui, "Ctrl+R", "Redo");
                    shortcut_row(ui, "P", "Preview mode");
                    shortcut_row(ui, "Ctrl+S", "Save scene");
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(16.0);

                // Column 3: Mode-specific
                ui.vertical(|ui| {
                    ui.set_min_width(220.0);

                    help_section(ui, "Hierarchy Mode (H)");
                    shortcut_row(ui, "/", "Search objects");
                    shortcut_row(ui, "Drag", "Reparent to group");
                    shortcut_row(ui, "Right Click", "Select children");

                    ui.add_space(12.0);
                    help_section(ui, "Inspector Mode (O)");
                    shortcut_row(ui, "/", "Search components");
                    shortcut_row(ui, "I", "Add component");
                    shortcut_row(ui, "X", "Remove component");
                    shortcut_row(ui, "N", "Focus name field");

                    ui.add_space(12.0);
                    help_section(ui, "Scene File");
                    shortcut_row(ui, "Ctrl+S", "Save");
                    shortcut_row(ui, "Ctrl+Shift+S", "Save As");
                    shortcut_row(ui, "Ctrl+O", "Open");
                    shortcut_row(ui, "Ctrl+N", "New scene");

                    ui.add_space(12.0);
                    help_section(ui, "Physics");
                    ui.label(
                        egui::RichText::new("Use command palette (C):")
                            .small()
                            .color(colors::TEXT_MUTED),
                    );
                    ui.label(
                        egui::RichText::new("  Toggle Physics")
                            .small()
                            .color(colors::TEXT_SECONDARY),
                    );
                    ui.label(
                        egui::RichText::new("  Toggle Physics Debug")
                            .small()
                            .color(colors::TEXT_SECONDARY),
                    );
                    ui.label(
                        egui::RichText::new("  Toggle Grid")
                            .small()
                            .color(colors::TEXT_SECONDARY),
                    );
                });
            });

            ui.add_space(8.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Press ? or use command palette to open this help")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Close").clicked() {
                        should_close = true;
                    }
                });
            });
        });

    // Handle Escape to close
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        should_close = true;
    }

    if should_close {
        state.open = false;
    }

    Ok(())
}

fn help_section(ui: &mut egui::Ui, title: &str) {
    ui.label(
        egui::RichText::new(title)
            .strong()
            .size(14.0)
            .color(colors::TEXT_PRIMARY),
    );
    ui.add_space(4.0);
}

fn shortcut_row(ui: &mut egui::Ui, key: &str, description: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("{:14}", key))
                .monospace()
                .strong()
                .color(colors::ACCENT_ORANGE),
        );
        ui.label(egui::RichText::new(description).color(colors::TEXT_SECONDARY));
    });
}

/// Draw dialog for setting a custom named camera mark
fn draw_custom_mark_dialog(
    mut contexts: EguiContexts,
    mut state: ResMut<CustomMarkDialogState>,
    mut set_mark_events: MessageWriter<SetCameraMarkEvent>,
    editor_state: Res<EditorState>,
) -> Result {
    // Don't draw UI when editor is disabled
    if !editor_state.ui_enabled {
        return Ok(());
    }

    if !state.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let mut should_close = false;
    let mut should_save = false;

    // Check for keyboard input before rendering UI
    let enter_pressed = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));

    if enter_pressed && !state.name.trim().is_empty() {
        should_save = true;
        should_close = true;
    }

    if escape_pressed {
        should_close = true;
    }

    egui::Window::new("Set Camera Mark")
        .collapsible(false)
        .resizable(false)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("Enter a name for this camera mark:").color(colors::TEXT_SECONDARY));
            ui.add_space(8.0);

            let response = ui.add(
                egui::TextEdit::singleline(&mut state.name)
                    .hint_text("Mark name...")
                    .desired_width(200.0),
            );

            if state.just_opened {
                response.request_focus();
                state.just_opened = false;
            }

            ui.add_space(8.0);

            ui.horizontal(|ui| {
                if ui.button("Cancel").clicked() {
                    should_close = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add_enabled(!state.name.trim().is_empty(), egui::Button::new("Save"))
                        .clicked()
                    {
                        should_save = true;
                        should_close = true;
                    }
                });
            });
        });

    if should_save {
        set_mark_events.write(SetCameraMarkEvent {
            name: state.name.trim().to_string(),
        });
    }

    if should_close {
        state.open = false;
    }

    Ok(())
}

/// Wrapper for component info to implement PaletteItem
struct ComponentSearchItem {
    type_id: TypeId,
    name: String,
}

impl super::fuzzy_palette::PaletteItem for ComponentSearchItem {
    fn label(&self) -> &str {
        &self.name
    }
}

/// Draw the component search palette for ObjectInspector mode
fn draw_component_search_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    component_editor_state: &mut ResMut<super::inspector::ComponentEditorState>,
    type_registry: &Res<AppTypeRegistry>,
    selected: &Query<Entity, With<Selected>>,
) -> Result {
    use super::fuzzy_palette::{draw_fuzzy_palette, PaletteConfig, PaletteResult, PaletteState};

    // Get selected entity
    let Some(_entity) = selected.iter().next() else {
        state.open = false;
        return Ok(());
    };

    // Build list of components from the type registry
    let type_registry_guard = type_registry.read();
    let mut components: Vec<ComponentSearchItem> = type_registry_guard
        .iter()
        .filter_map(|registration| {
            registration.data::<ReflectComponent>()?;
            let type_id = registration.type_id();
            let short_name = registration
                .type_info()
                .type_path_table()
                .short_path()
                .to_string();
            Some(ComponentSearchItem { type_id, name: short_name })
        })
        .collect();
    components.sort_by(|a, b| a.name.cmp(&b.name));
    drop(type_registry_guard);

    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState {
        query: std::mem::take(&mut state.query),
        selected_index: state.selected_index,
        just_opened: state.just_opened,
    };

    let config = PaletteConfig {
        title: "INSPECT MODE",
        title_color: colors::ACCENT_PURPLE,
        subtitle: "Search for component to edit",
        hint_text: "Type to search components...",
        action_label: "edit",
        size: [400.0, 300.0],
        show_categories: false,
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &components, &config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            let item = &components[index];
            component_editor_state.editing_component = Some((item.type_id, item.name.clone()));
            component_editor_state.just_opened = true;
            state.open = false;
        }
        PaletteResult::Closed => {
            state.open = false;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}

/// Wrapper for adding components that implements PaletteItem
struct AddComponentItem {
    type_id: TypeId,
    short_name: String,
    category: String,
    can_instantiate: bool,
}

impl super::fuzzy_palette::PaletteItem for AddComponentItem {
    fn label(&self) -> &str {
        &self.short_name
    }

    fn category(&self) -> Option<&str> {
        Some(&self.category)
    }

    fn is_enabled(&self) -> bool {
        self.can_instantiate
    }

    fn suffix(&self) -> Option<&str> {
        if self.can_instantiate {
            None
        } else {
            Some("(no default)")
        }
    }
}

/// Draw the add component palette for adding new components to an entity
fn draw_add_component_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    component_registry: &mut ResMut<ComponentRegistry>,
    type_registry: &Res<AppTypeRegistry>,
    commands: &mut Commands,
) -> Result {
    use super::fuzzy_palette::{draw_fuzzy_palette, PaletteConfig, PaletteResult, PaletteState};

    // Get target entity
    let Some(target_entity) = state.target_entity else {
        state.open = false;
        return Ok(());
    };

    // Ensure registry is populated
    {
        let type_registry_guard = type_registry.read();
        component_registry.populate(&type_registry_guard);
    }

    // Convert to PaletteItem wrappers
    let items: Vec<AddComponentItem> = component_registry
        .components
        .iter()
        .map(|c| AddComponentItem {
            type_id: c.type_id,
            short_name: c.short_name.clone(),
            category: c.category.clone(),
            can_instantiate: c.can_instantiate,
        })
        .collect();

    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState {
        query: std::mem::take(&mut state.query),
        selected_index: state.selected_index,
        just_opened: state.just_opened,
    };

    let config = PaletteConfig {
        title: "ADD COMPONENT",
        title_color: colors::ACCENT_GREEN,
        subtitle: "Select component to add",
        hint_text: "Type to search components...",
        action_label: "add",
        size: [400.0, 350.0],
        show_categories: true,
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &items, &config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            let item = &items[index];
            // Queue a command to add the component
            commands.queue(AddComponentCommand {
                entity: target_entity,
                type_id: item.type_id,
                component_name: item.short_name.clone(),
            });
            state.open = false;
            state.target_entity = None;
        }
        PaletteResult::Closed => {
            state.open = false;
            state.target_entity = None;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}

/// Command to add a component via reflection (deferred execution)
struct AddComponentCommand {
    entity: Entity,
    type_id: TypeId,
    component_name: String,
}

impl bevy::prelude::Command for AddComponentCommand {
    fn apply(self, world: &mut World) {
        if add_component_by_type_id(world, self.entity, self.type_id) {
            // Open the component editor for the newly added component
            let mut editor_state = world.resource_mut::<super::inspector::ComponentEditorState>();
            editor_state.editing_component = Some((self.type_id, self.component_name));
            editor_state.just_opened = true;
        }
    }
}

/// Item for the remove component palette
struct RemoveComponentItem {
    type_id: TypeId,
    short_name: String,
}

impl super::fuzzy_palette::PaletteItem for RemoveComponentItem {
    fn label(&self) -> &str {
        &self.short_name
    }

    fn category(&self) -> Option<&str> {
        None
    }

    fn is_enabled(&self) -> bool {
        true
    }

    fn suffix(&self) -> Option<&str> {
        None
    }

    fn keywords(&self) -> &[String] {
        &[]
    }
}

/// Draw the remove component palette
fn draw_remove_component_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    removable_cache: &Res<RemovableComponentsCache>,
    selected: &Query<Entity, With<Selected>>,
    commands: &mut Commands,
) -> Result {
    use super::fuzzy_palette::{draw_fuzzy_palette, PaletteConfig, PaletteResult, PaletteState};

    // Get target entity (either from state or first selected)
    let target_entity = state.target_entity.or_else(|| selected.iter().next());
    let Some(target_entity) = target_entity else {
        state.open = false;
        return Ok(());
    };

    // Store in state so the populate_removable_components system can fill the cache
    state.target_entity = Some(target_entity);

    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState {
        query: std::mem::take(&mut state.query),
        selected_index: state.selected_index,
        just_opened: state.just_opened,
    };

    // Use the cached component list (populated by populate_removable_components system)
    let items: Vec<RemoveComponentItem> = removable_cache
        .components
        .iter()
        .map(|(type_id, name)| RemoveComponentItem {
            type_id: *type_id,
            short_name: name.clone(),
        })
        .collect();

    let config = PaletteConfig {
        title: "REMOVE COMPONENT",
        title_color: colors::STATUS_ERROR,
        subtitle: "Select component to remove",
        hint_text: "Type to search components...",
        action_label: "remove",
        size: [400.0, 350.0],
        show_categories: false,
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &items, &config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            if let Some(item) = items.get(index) {
                // Queue snapshot and remove commands
                commands.queue(TakeSnapshotCommand {
                    description: format!("Remove {} component", item.short_name),
                });
                commands.queue(RemoveComponentCommand {
                    entity: target_entity,
                    type_id: item.type_id,
                    component_name: item.short_name.clone(),
                });
            }
            state.open = false;
            state.target_entity = None;
        }
        PaletteResult::Closed => {
            state.open = false;
            state.target_entity = None;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}

/// Command to remove a component via reflection (deferred execution)
struct RemoveComponentCommand {
    entity: Entity,
    type_id: TypeId,
    component_name: String,
}

impl bevy::prelude::Command for RemoveComponentCommand {
    fn apply(self, world: &mut World) {
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry_guard = type_registry.read();

        // Find the component registration
        let Some(registration) = type_registry_guard.get(self.type_id) else {
            warn!("Cannot find type registration for component: {}", self.component_name);
            return;
        };

        // Get ReflectComponent to remove
        let Some(reflect_component) = registration.data::<ReflectComponent>() else {
            warn!("Component {} does not have ReflectComponent", self.component_name);
            return;
        };

        // Remove the component
        reflect_component.remove(&mut world.entity_mut(self.entity));
        info!("Removed component {} from entity {:?}", self.component_name, self.entity);
    }
}

/// Command to create a spline distribution from selected entities (deferred execution)
struct CreateDistributionCommand {
    selected_entities: Vec<Entity>,
}

impl bevy::prelude::Command for CreateDistributionCommand {
    fn apply(self, world: &mut World) {
        if self.selected_entities.len() != 2 {
            info!("Select exactly 2 entities: a spline and a source object");
            return;
        }

        // Determine which entity is the spline
        let has_spline_0 = world.get::<Spline>(self.selected_entities[0]).is_some();
        let has_spline_1 = world.get::<Spline>(self.selected_entities[1]).is_some();

        let (spline_entity, source_entity) = match (has_spline_0, has_spline_1) {
            (true, false) => (self.selected_entities[0], self.selected_entities[1]),
            (false, true) => (self.selected_entities[1], self.selected_entities[0]),
            (true, true) => {
                info!("Both selected entities are splines. Select one spline and one source object.");
                return;
            }
            (false, false) => {
                info!("Neither selected entity is a spline. Select one spline and one source object.");
                return;
            }
        };

        // Create the distribution entity
        world.spawn((
            SplineDistribution::new(spline_entity, source_entity, 10)
                .with_orientation(DistributionOrientation::align_to_tangent())
                .uniform(),
            Name::new("Distribution"),
        ));

        // Mark source as DistributionSource (hides it from rendering)
        world.entity_mut(source_entity).insert(DistributionSource);

        info!("Created distribution with 10 instances along spline");
    }
}
