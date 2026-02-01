use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::any::TypeId;

use crate::commands::{RedoEvent, UndoEvent};
use crate::editor::{
    CameraMarks, EditorMode, EditorState, InsertObjectType, JumpToLastPositionEvent,
    JumpToMarkEvent, SetCameraMarkEvent, StartInsertEvent, ToggleGridEvent, TogglePhysicsDebugEvent,
    TogglePhysicsEvent,
};
use crate::scene::{
    LoadSceneEvent, PrimitiveShape, SaveSceneEvent, SpawnDemoSceneEvent, SpawnDirectionalLightEvent,
    SpawnGroupEvent, SpawnPointLightEvent, SpawnPrimitiveEvent, UnparentSelectedEvent,
};
use crate::selection::Selected;
use crate::ui::component_browser::{add_component_by_type_id, ComponentBrowserState, ComponentRegistry};
use crate::ui::theme::colors;
use crate::ui::SettingsWindowState;

/// System parameter grouping all command palette event writers
#[derive(SystemParam)]
struct CommandEvents<'w> {
    spawn_primitive: MessageWriter<'w, SpawnPrimitiveEvent>,
    spawn_group: MessageWriter<'w, SpawnGroupEvent>,
    spawn_light: MessageWriter<'w, SpawnPointLightEvent>,
    spawn_directional_light: MessageWriter<'w, SpawnDirectionalLightEvent>,
    unparent: MessageWriter<'w, UnparentSelectedEvent>,
    set_mark: MessageWriter<'w, SetCameraMarkEvent>,
    jump_mark: MessageWriter<'w, JumpToMarkEvent>,
    jump_last: MessageWriter<'w, JumpToLastPositionEvent>,
    save_scene: MessageWriter<'w, SaveSceneEvent>,
    load_scene: MessageWriter<'w, LoadSceneEvent>,
    toggle_debug: MessageWriter<'w, TogglePhysicsDebugEvent>,
    toggle_physics: MessageWriter<'w, TogglePhysicsEvent>,
    toggle_grid: MessageWriter<'w, ToggleGridEvent>,
    start_insert: MessageWriter<'w, StartInsertEvent>,
    spawn_demo: MessageWriter<'w, SpawnDemoSceneEvent>,
    undo: MessageWriter<'w, UndoEvent>,
    redo: MessageWriter<'w, RedoEvent>,
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

/// Open the command palette in AddComponent mode for a specific entity
pub fn open_add_component_palette(state: &mut CommandPaletteState, entity: Entity) {
    state.open = true;
    state.query.clear();
    state.selected_index = 0;
    state.just_opened = true;
    state.mode = PaletteMode::AddComponent;
    state.target_entity = Some(entity);
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
            .insert_resource(registry)
            .add_systems(Update, handle_palette_toggle)
            .add_systems(EguiPrimaryContextPass, (draw_command_palette, draw_help_window, draw_custom_mark_dialog));
    }
}

/// Open palette with C key, or / key for component search in ObjectInspector mode
fn handle_palette_toggle(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<CommandPaletteState>,
    mut registry: ResMut<CommandRegistry>,
    marks: Res<CameraMarks>,
    editor_mode: Res<State<EditorMode>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
) {
    // Don't open when editor is disabled
    if !editor_state.editor_active {
        return;
    }

    // Don't open if already open or UI wants keyboard input
    if state.open {
        return;
    }

    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }

    // "/" key opens component search in ObjectInspector mode
    if keyboard.just_pressed(KeyCode::Slash) && *editor_mode.get() == EditorMode::ObjectInspector {
        state.open = true;
        state.query.clear();
        state.selected_index = 0;
        state.just_opened = true;
        state.mode = PaletteMode::ComponentSearch;
        return;
    }

    if keyboard.just_pressed(KeyCode::KeyC) {
        state.open = true;
        state.query.clear();
        state.selected_index = 0;
        state.just_opened = true;
        state.mode = PaletteMode::Commands;
        // Refresh dynamic commands
        registry.add_mark_commands(&marks);
    }
}

/// Draw the command palette
fn draw_command_palette(
    mut contexts: EguiContexts,
    mut state: ResMut<CommandPaletteState>,
    mut help_state: ResMut<HelpWindowState>,
    mut settings_state: ResMut<SettingsWindowState>,
    mut custom_mark_state: ResMut<CustomMarkDialogState>,
    mut editor_state: ResMut<EditorState>,
    _browser_state: ResMut<ComponentBrowserState>,
    mut component_editor_state: ResMut<super::inspector::ComponentEditorState>,
    mut component_registry: ResMut<ComponentRegistry>,
    registry: Res<CommandRegistry>,
    type_registry: Res<AppTypeRegistry>,
    _mode: Res<State<EditorMode>>,
    selected: Query<Entity, With<Selected>>,
    mut events: CommandEvents,
    mut commands: Commands,
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
            &mut component_editor_state,
            &type_registry,
            &selected,
        );
    }

    // Handle AddComponent mode separately
    if state.mode == PaletteMode::AddComponent {
        return draw_add_component_palette(
            ctx,
            &mut state,
            &mut component_registry,
            &type_registry,
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
            let object_type = match &action {
                CommandAction::SpawnPrimitive(shape) => Some(InsertObjectType::Primitive(*shape)),
                CommandAction::SpawnPointLight => Some(InsertObjectType::PointLight),
                CommandAction::SpawnDirectionalLight => Some(InsertObjectType::DirectionalLight),
                CommandAction::SpawnGroup => Some(InsertObjectType::Group),
                _ => None,
            };

            if let Some(obj_type) = object_type {
                events.start_insert.write(StartInsertEvent {
                    object_type: obj_type,
                });
            }
        } else {
            // Normal mode - execute action immediately
            match action {
                CommandAction::SpawnPrimitive(shape) => {
                    events.spawn_primitive.write(SpawnPrimitiveEvent {
                        shape,
                        position: Vec3::ZERO,
                    });
                }
                CommandAction::SpawnPointLight => {
                    events.spawn_light.write(SpawnPointLightEvent {
                        position: Vec3::new(0.0, 3.0, 0.0),
                    });
                }
                CommandAction::SpawnDirectionalLight => {
                    events.spawn_directional_light.write(SpawnDirectionalLightEvent {
                        position: Vec3::new(4.0, 8.0, 4.0),
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
                    // For now, save to a default location
                    events.save_scene.write(SaveSceneEvent {
                        path: "scene.ron".to_string(),
                    });
                }
                CommandAction::LoadScene => {
                    // For now, load from a default location
                    events.load_scene.write(LoadSceneEvent {
                        path: "scene.ron".to_string(),
                    });
                }
                CommandAction::ShowHelp => {
                    help_state.open = true;
                }
                CommandAction::OpenSettings => {
                    settings_state.open = true;
                }
                CommandAction::SetGridSnap(value) => {
                    editor_state.grid_snap = value;
                }
                CommandAction::SetRotationSnap(value) => {
                    editor_state.rotation_snap = value;
                }
                CommandAction::ShowCustomMarkDialog => {
                    custom_mark_state.open = true;
                    custom_mark_state.name.clear();
                    custom_mark_state.just_opened = true;
                }
                CommandAction::SpawnGroup => {
                    events.spawn_group.write(SpawnGroupEvent {
                        position: Vec3::ZERO,
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
                        state.query.clear();
                        state.selected_index = 0;
                        state.just_opened = true;
                        state.mode = PaletteMode::AddComponent;
                        state.target_entity = Some(entity);
                        // Don't close the palette, we're switching modes
                        should_close = false;
                    }
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
    // Get selected entity
    let Some(_entity) = selected.iter().next() else {
        // No entity selected, close the palette
        state.open = false;
        return Ok(());
    };

    // Build list of components from the type registry
    // Note: In a full implementation, we'd want to filter to only components on the entity
    let type_registry_guard = type_registry.read();

    let mut components: Vec<ComponentSearchItem> = type_registry_guard
        .iter()
        .filter_map(|registration| {
            // Only include types with ReflectComponent
            registration.data::<ReflectComponent>()?;

            let type_id = registration.type_id();
            let short_name = registration
                .type_info()
                .type_path_table()
                .short_path()
                .to_string();

            Some(ComponentSearchItem {
                type_id,
                name: short_name,
            })
        })
        .collect();

    components.sort_by(|a, b| a.name.cmp(&b.name));
    drop(type_registry_guard);

    // Filter using fuzzy matching
    let filtered = super::fuzzy_palette::fuzzy_filter(&components, &state.query);

    // Clamp selected index
    if !filtered.is_empty() {
        state.selected_index = state.selected_index.min(filtered.len() - 1);
    }

    let mut should_close = false;
    let mut selected_component: Option<(TypeId, String)> = None;

    // Check for keyboard input before rendering UI
    let enter_pressed = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let down_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
    let up_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));

    // Handle Enter to select component
    if enter_pressed && !filtered.is_empty() {
        if let Some(filtered_item) = filtered.get(state.selected_index) {
            selected_component = Some((filtered_item.item.type_id, filtered_item.item.name.clone()));
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

    egui::Window::new("Search Components")
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([400.0, 300.0])
        .show(ctx, |ui| {
            // Mode indicator
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("INSPECT MODE")
                        .small()
                        .strong()
                        .color(colors::ACCENT_PURPLE),
                );
                ui.label(
                    egui::RichText::new("- Search for component to edit")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
            });
            ui.add_space(4.0);

            // Search input
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Type to search components...")
                    .desired_width(f32::INFINITY),
            );

            // Focus the input when just opened
            if state.just_opened {
                response.request_focus();
                state.just_opened = false;
            }

            ui.separator();

            // Component list
            egui::ScrollArea::vertical()
                .max_height(250.0)
                .show(ui, |ui| {
                    if filtered.is_empty() {
                        ui.label(egui::RichText::new("No matching components").color(colors::TEXT_MUTED));
                    } else {
                        for (display_idx, filtered_item) in filtered.iter().enumerate() {
                            let is_selected = display_idx == state.selected_index;
                            let text_color = if is_selected {
                                colors::TEXT_PRIMARY
                            } else {
                                colors::TEXT_SECONDARY
                            };

                            let response = ui.selectable_label(
                                is_selected,
                                egui::RichText::new(filtered_item.item.name.as_str()).color(text_color),
                            );

                            if response.clicked() {
                                selected_component = Some((filtered_item.item.type_id, filtered_item.item.name.clone()));
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
                ui.label(egui::RichText::new("to edit").small().color(colors::TEXT_MUTED));
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Esc").small().strong().color(colors::ACCENT_BLUE));
                ui.label(egui::RichText::new("to close").small().color(colors::TEXT_MUTED));
            });
        });

    // Handle selected component
    if let Some((type_id, name)) = selected_component {
        component_editor_state.editing_component = Some((type_id, name));
        component_editor_state.just_opened = true;
    }

    if should_close {
        state.open = false;
    }

    Ok(())
}

/// Draw the add component palette for adding new components to an entity
fn draw_add_component_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    component_registry: &mut ResMut<ComponentRegistry>,
    type_registry: &Res<AppTypeRegistry>,
    commands: &mut Commands,
) -> Result {
    // Get target entity
    let Some(target_entity) = state.target_entity else {
        // No target entity, close the palette
        state.open = false;
        return Ok(());
    };

    // Ensure registry is populated
    {
        let type_registry_guard = type_registry.read();
        component_registry.populate(&type_registry_guard);
    }

    // Filter components
    let filtered = component_registry.filter(&state.query);

    // Clamp selected index
    if !filtered.is_empty() {
        state.selected_index = state.selected_index.min(filtered.len() - 1);
    }

    let mut should_close = false;
    let mut component_to_add: Option<TypeId> = None;

    // Check for keyboard input before rendering UI
    let enter_pressed = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let down_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
    let up_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));

    // Handle Enter to add component
    if enter_pressed && !filtered.is_empty() {
        if let Some((_, info, _)) = filtered.get(state.selected_index) {
            if info.can_instantiate {
                component_to_add = Some(info.type_id);
                should_close = true;
            }
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

    egui::Window::new("Add Component")
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([400.0, 350.0])
        .show(ctx, |ui| {
            // Mode indicator
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("ADD COMPONENT")
                        .small()
                        .strong()
                        .color(colors::ACCENT_GREEN),
                );
                ui.label(
                    egui::RichText::new("- Select component to add")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
            });
            ui.add_space(4.0);

            // Search input
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Type to search components...")
                    .desired_width(f32::INFINITY),
            );

            // Focus the input when just opened
            if state.just_opened {
                response.request_focus();
                state.just_opened = false;
            }

            ui.separator();

            // Component list
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| {
                    if filtered.is_empty() {
                        ui.label(egui::RichText::new("No matching components").color(colors::TEXT_MUTED));
                    } else {
                        let mut current_category: Option<&str> = None;

                        for (display_idx, (_, info, _)) in filtered.iter().enumerate() {
                            // Category header
                            if current_category != Some(&info.category) {
                                current_category = Some(&info.category);
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(&info.category)
                                        .small()
                                        .color(colors::TEXT_MUTED),
                                );
                            }

                            let is_selected = display_idx == state.selected_index;
                            let can_add = info.can_instantiate;

                            let text_color = if !can_add {
                                colors::TEXT_MUTED
                            } else if is_selected {
                                colors::TEXT_PRIMARY
                            } else {
                                colors::TEXT_SECONDARY
                            };

                            let label_text = if can_add {
                                info.short_name.clone()
                            } else {
                                format!("{} (no default)", info.short_name)
                            };

                            let response = ui.selectable_label(
                                is_selected,
                                egui::RichText::new(&label_text).color(text_color),
                            );

                            if response.clicked() && can_add {
                                component_to_add = Some(info.type_id);
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
                ui.label(egui::RichText::new("to add").small().color(colors::TEXT_MUTED));
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Esc").small().strong().color(colors::ACCENT_BLUE));
                ui.label(egui::RichText::new("to close").small().color(colors::TEXT_MUTED));
            });
        });

    // Handle adding component - we need to defer this since we need world access
    if let Some(type_id) = component_to_add {
        // Get the component name for the editor popup
        let component_name = component_registry
            .components
            .iter()
            .find(|c| c.type_id == type_id)
            .map(|c| c.short_name.clone())
            .unwrap_or_else(|| "Component".to_string());

        // Store the component to add in a command that will run later
        commands.queue(AddComponentCommand {
            entity: target_entity,
            type_id,
            component_name,
        });
    }

    if should_close {
        state.open = false;
        state.target_entity = None;
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
