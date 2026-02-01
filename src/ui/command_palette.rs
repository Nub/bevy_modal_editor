use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use crate::editor::{
    CameraMarks, EditorState, JumpToLastPositionEvent, JumpToMarkEvent, SetCameraMarkEvent,
};
use crate::scene::{
    LoadSceneEvent, PrimitiveShape, SaveSceneEvent, SpawnGroupEvent, SpawnPrimitiveEvent,
    UnparentEvent,
};
use crate::selection::Selected;

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
}

/// Actions that commands can perform
#[derive(Clone)]
pub enum CommandAction {
    SpawnPrimitive(PrimitiveShape),
    SetCameraMark(String),
    JumpToMark(String),
    JumpToLastPosition,
    SaveScene,
    LoadScene,
    ShowHelp,
    SetGridSnap(f32),
    SetRotationSnap(f32),
    ShowCustomMarkDialog,
    SpawnGroup,
    UnparentSelected,
}

/// Resource to track command palette state
#[derive(Resource)]
pub struct CommandPaletteState {
    pub open: bool,
    pub query: String,
    pub selected_index: usize,
    /// Whether we just opened (to focus the input)
    pub just_opened: bool,
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
        }
    }
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

        // Primitive spawning
        self.commands.push(Command {
            name: "Add Cube".to_string(),
            keywords: vec!["box".into(), "primitive".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Cube),
        });
        self.commands.push(Command {
            name: "Add Sphere".to_string(),
            keywords: vec!["ball".into(), "primitive".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Sphere),
        });
        self.commands.push(Command {
            name: "Add Cylinder".to_string(),
            keywords: vec!["tube".into(), "primitive".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Cylinder),
        });
        self.commands.push(Command {
            name: "Add Capsule".to_string(),
            keywords: vec!["pill".into(), "primitive".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Capsule),
        });
        self.commands.push(Command {
            name: "Add Plane".to_string(),
            keywords: vec!["floor".into(), "ground".into(), "primitive".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Plane),
        });

        // Scene operations
        self.commands.push(Command {
            name: "Save Scene".to_string(),
            keywords: vec!["export".into(), "file".into()],
            category: "Scene",
            action: CommandAction::SaveScene,
        });
        self.commands.push(Command {
            name: "Load Scene".to_string(),
            keywords: vec!["import".into(), "open".into(), "file".into()],
            category: "Scene",
            action: CommandAction::LoadScene,
        });

        // Groups
        self.commands.push(Command {
            name: "Add Group".to_string(),
            keywords: vec!["folder".into(), "container".into(), "nest".into()],
            category: "Primitives",
            action: CommandAction::SpawnGroup,
        });
        self.commands.push(Command {
            name: "Unparent Selected".to_string(),
            keywords: vec!["detach".into(), "remove".into(), "parent".into()],
            category: "Hierarchy",
            action: CommandAction::UnparentSelected,
        });

        // Camera marks
        self.commands.push(Command {
            name: "Jump to Last Position".to_string(),
            keywords: vec!["back".into(), "previous".into(), "camera".into()],
            category: "Camera",
            action: CommandAction::JumpToLastPosition,
        });
        self.commands.push(Command {
            name: "Set Custom Camera Mark".to_string(),
            keywords: vec!["save".into(), "bookmark".into(), "name".into(), "camera".into()],
            category: "Camera",
            action: CommandAction::ShowCustomMarkDialog,
        });

        // Help
        self.commands.push(Command {
            name: "Help: Keyboard Shortcuts".to_string(),
            keywords: vec!["hotkeys".into(), "keys".into(), "bindings".into(), "controls".into()],
            category: "Help",
            action: CommandAction::ShowHelp,
        });

        // Grid snap
        self.commands.push(Command {
            name: "Grid Snap: Off".to_string(),
            keywords: vec!["disable".into(), "none".into()],
            category: "Snapping",
            action: CommandAction::SetGridSnap(0.0),
        });
        self.commands.push(Command {
            name: "Grid Snap: 0.25".to_string(),
            keywords: vec!["quarter".into()],
            category: "Snapping",
            action: CommandAction::SetGridSnap(0.25),
        });
        self.commands.push(Command {
            name: "Grid Snap: 0.5".to_string(),
            keywords: vec!["half".into()],
            category: "Snapping",
            action: CommandAction::SetGridSnap(0.5),
        });
        self.commands.push(Command {
            name: "Grid Snap: 1.0".to_string(),
            keywords: vec!["one".into(), "unit".into()],
            category: "Snapping",
            action: CommandAction::SetGridSnap(1.0),
        });
        self.commands.push(Command {
            name: "Grid Snap: 2.0".to_string(),
            keywords: vec!["two".into()],
            category: "Snapping",
            action: CommandAction::SetGridSnap(2.0),
        });

        // Rotation snap
        self.commands.push(Command {
            name: "Rotation Snap: Off".to_string(),
            keywords: vec!["angle".into(), "disable".into(), "none".into()],
            category: "Snapping",
            action: CommandAction::SetRotationSnap(0.0),
        });
        self.commands.push(Command {
            name: "Rotation Snap: 15°".to_string(),
            keywords: vec!["angle".into(), "degrees".into()],
            category: "Snapping",
            action: CommandAction::SetRotationSnap(15.0),
        });
        self.commands.push(Command {
            name: "Rotation Snap: 45°".to_string(),
            keywords: vec!["angle".into(), "degrees".into()],
            category: "Snapping",
            action: CommandAction::SetRotationSnap(45.0),
        });
        self.commands.push(Command {
            name: "Rotation Snap: 90°".to_string(),
            keywords: vec!["angle".into(), "degrees".into(), "right".into()],
            category: "Snapping",
            action: CommandAction::SetRotationSnap(90.0),
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
            });
        }

        // Add set mark commands for quick marks 1-9
        for i in 1..=9 {
            self.commands.push(Command {
                name: format!("Set Mark {}", i),
                keywords: vec!["save".into(), "camera".into()],
                category: "Camera Marks",
                action: CommandAction::SetCameraMark(i.to_string()),
            });
        }
    }
}

/// Get filtered and sorted commands based on query using skim fuzzy matcher
fn filter_commands<'a>(commands: &'a [Command], query: &str) -> Vec<(usize, &'a Command, i64)> {
    let matcher = SkimMatcherV2::default();

    if query.is_empty() {
        // Return all commands with score 0 when no query
        return commands
            .iter()
            .enumerate()
            .map(|(idx, cmd)| (idx, cmd, 0i64))
            .collect();
    }

    let mut results: Vec<(usize, &Command, i64)> = commands
        .iter()
        .enumerate()
        .filter_map(|(idx, cmd)| {
            // Check name first
            if let Some(score) = matcher.fuzzy_match(&cmd.name, query) {
                return Some((idx, cmd, score));
            }

            // Check keywords - find best match
            let best_keyword_score = cmd.keywords.iter()
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

/// Open palette with C key
fn handle_palette_toggle(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<CommandPaletteState>,
    mut registry: ResMut<CommandRegistry>,
    marks: Res<CameraMarks>,
    mut contexts: EguiContexts,
) {
    // Don't open if already open or UI wants keyboard input
    if state.open {
        return;
    }

    if let Ok(ctx) = contexts.ctx_mut() {
        if ctx.wants_keyboard_input() {
            return;
        }
    }

    if keyboard.just_pressed(KeyCode::KeyC) {
        state.open = true;
        state.query.clear();
        state.selected_index = 0;
        state.just_opened = true;
        // Refresh dynamic commands
        registry.add_mark_commands(&marks);
    }
}

/// Draw the command palette
fn draw_command_palette(
    mut contexts: EguiContexts,
    mut state: ResMut<CommandPaletteState>,
    mut help_state: ResMut<HelpWindowState>,
    mut custom_mark_state: ResMut<CustomMarkDialogState>,
    mut editor_state: ResMut<EditorState>,
    registry: Res<CommandRegistry>,
    mut spawn_events: MessageWriter<SpawnPrimitiveEvent>,
    mut spawn_group_events: MessageWriter<SpawnGroupEvent>,
    mut unparent_events: MessageWriter<UnparentEvent>,
    mut set_mark_events: MessageWriter<SetCameraMarkEvent>,
    mut jump_mark_events: MessageWriter<JumpToMarkEvent>,
    mut jump_last_events: MessageWriter<JumpToLastPositionEvent>,
    mut save_events: MessageWriter<SaveSceneEvent>,
    mut load_events: MessageWriter<LoadSceneEvent>,
    selected: Query<Entity, With<Selected>>,
) -> Result {
    if !state.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let filtered = filter_commands(&registry.commands, &state.query);

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

    egui::Window::new("Command Palette")
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .anchor(egui::Align2::CENTER_TOP, [0.0, 100.0])
        .fixed_size([400.0, 300.0])
        .show(ctx, |ui| {
            // Search input
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Type to search commands...")
                    .desired_width(f32::INFINITY)
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
                        ui.label("No matching commands");
                    } else {
                        let mut current_category: Option<&str> = None;

                        for (display_idx, (_, cmd, _)) in filtered.iter().enumerate() {
                            // Category header
                            if current_category != Some(cmd.category) {
                                current_category = Some(cmd.category);
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(cmd.category).small().color(egui::Color32::GRAY));
                            }

                            let is_selected = display_idx == state.selected_index;

                            let response = ui.selectable_label(is_selected, &cmd.name);

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
                ui.label(egui::RichText::new("Enter").small().strong());
                ui.label(egui::RichText::new("to select").small());
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Esc").small().strong());
                ui.label(egui::RichText::new("to close").small());
            });
        });

    // Execute action after UI
    if let Some(action) = action_to_execute {
        match action {
            CommandAction::SpawnPrimitive(shape) => {
                spawn_events.write(SpawnPrimitiveEvent {
                    shape,
                    position: Vec3::ZERO,
                });
            }
            CommandAction::SetCameraMark(name) => {
                set_mark_events.write(SetCameraMarkEvent { name });
            }
            CommandAction::JumpToMark(name) => {
                jump_mark_events.write(JumpToMarkEvent { name });
            }
            CommandAction::JumpToLastPosition => {
                jump_last_events.write(JumpToLastPositionEvent);
            }
            CommandAction::SaveScene => {
                // For now, save to a default location
                save_events.write(SaveSceneEvent {
                    path: "scene.ron".to_string(),
                });
            }
            CommandAction::LoadScene => {
                // For now, load from a default location
                load_events.write(LoadSceneEvent {
                    path: "scene.ron".to_string(),
                });
            }
            CommandAction::ShowHelp => {
                help_state.open = true;
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
                spawn_group_events.write(SpawnGroupEvent {
                    position: Vec3::ZERO,
                });
            }
            CommandAction::UnparentSelected => {
                if let Ok(entity) = selected.single() {
                    unparent_events.write(UnparentEvent { entity });
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
fn draw_help_window(mut contexts: EguiContexts, mut state: ResMut<HelpWindowState>) -> Result {
    if !state.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let mut should_close = false;

    egui::Window::new("Keyboard Shortcuts")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_min_width(350.0);

            // General
            ui.heading("General");
            ui.add_space(4.0);
            shortcut_row(ui, "C", "Open command palette");
            shortcut_row(ui, "F", "Find object in scene");
            shortcut_row(ui, "V", "Toggle View/Edit mode");
            shortcut_row(ui, "Esc", "Return to View mode / Cancel");

            ui.add_space(12.0);
            ui.heading("View Mode - Camera");
            ui.add_space(4.0);
            shortcut_row(ui, "W/A/S/D", "Move camera");
            shortcut_row(ui, "Space/Ctrl", "Move up/down");
            shortcut_row(ui, "Shift", "Move faster");
            shortcut_row(ui, "Right Mouse", "Look around");
            shortcut_row(ui, "1-9", "Jump to camera mark");
            shortcut_row(ui, "Shift+1-9", "Set camera mark");
            shortcut_row(ui, "`", "Jump to last position");

            ui.add_space(12.0);
            ui.heading("View Mode - Selection");
            ui.add_space(4.0);
            shortcut_row(ui, "Left Click", "Select object");
            shortcut_row(ui, "Shift+Click", "Multi-select");
            shortcut_row(ui, "G", "Group selected objects");
            shortcut_row(ui, "Delete", "Delete selected");

            ui.add_space(12.0);
            ui.heading("Edit Mode - Transform");
            ui.add_space(4.0);
            shortcut_row(ui, "Q", "Translate tool");
            shortcut_row(ui, "W", "Rotate tool");
            shortcut_row(ui, "E", "Scale tool");
            shortcut_row(ui, "A", "Constrain to X axis");
            shortcut_row(ui, "S", "Constrain to Y axis");
            shortcut_row(ui, "D", "Constrain to Z axis");
            shortcut_row(ui, "J/K", "Step transform -/+");

            ui.add_space(16.0);
            ui.separator();
            ui.add_space(4.0);

            ui.horizontal(|ui| {
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

fn shortcut_row(ui: &mut egui::Ui, key: &str, description: &str) {
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(key)
                .monospace()
                .strong()
                .color(egui::Color32::from_rgb(200, 200, 100)),
        );
        ui.label(description);
    });
}

/// Draw dialog for setting a custom named camera mark
fn draw_custom_mark_dialog(
    mut contexts: EguiContexts,
    mut state: ResMut<CustomMarkDialogState>,
    mut set_mark_events: MessageWriter<SetCameraMarkEvent>,
) -> Result {
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
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label("Enter a name for this camera mark:");
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
