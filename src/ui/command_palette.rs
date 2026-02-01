use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use crate::editor::{
    CameraMarks, JumpToLastPositionEvent, JumpToMarkEvent, SetCameraMarkEvent,
};
use crate::scene::{LoadSceneEvent, PrimitiveShape, SaveSceneEvent, SpawnPrimitiveEvent};

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
            keywords: vec!["add".into(), "cube".into(), "box".into(), "primitive".into(), "spawn".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Cube),
        });
        self.commands.push(Command {
            name: "Add Sphere".to_string(),
            keywords: vec!["add".into(), "sphere".into(), "ball".into(), "primitive".into(), "spawn".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Sphere),
        });
        self.commands.push(Command {
            name: "Add Cylinder".to_string(),
            keywords: vec!["add".into(), "cylinder".into(), "tube".into(), "primitive".into(), "spawn".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Cylinder),
        });
        self.commands.push(Command {
            name: "Add Capsule".to_string(),
            keywords: vec!["add".into(), "capsule".into(), "pill".into(), "primitive".into(), "spawn".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Capsule),
        });
        self.commands.push(Command {
            name: "Add Plane".to_string(),
            keywords: vec!["add".into(), "plane".into(), "floor".into(), "ground".into(), "primitive".into(), "spawn".into()],
            category: "Primitives",
            action: CommandAction::SpawnPrimitive(PrimitiveShape::Plane),
        });

        // Scene operations
        self.commands.push(Command {
            name: "Save Scene".to_string(),
            keywords: vec!["save".into(), "scene".into(), "file".into(), "export".into()],
            category: "Scene",
            action: CommandAction::SaveScene,
        });
        self.commands.push(Command {
            name: "Load Scene".to_string(),
            keywords: vec!["load".into(), "scene".into(), "file".into(), "import".into(), "open".into()],
            category: "Scene",
            action: CommandAction::LoadScene,
        });

        // Camera marks
        self.commands.push(Command {
            name: "Jump to Last Position".to_string(),
            keywords: vec!["jump".into(), "last".into(), "camera".into(), "position".into(), "back".into()],
            category: "Camera",
            action: CommandAction::JumpToLastPosition,
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
                keywords: vec!["jump".into(), "mark".into(), "camera".into(), name.to_lowercase()],
                category: "Camera Marks",
                action: CommandAction::JumpToMark(name.clone()),
            });
        }

        // Add set mark commands for quick marks 1-9
        for i in 1..=9 {
            self.commands.push(Command {
                name: format!("Set Mark {}", i),
                keywords: vec!["set".into(), "mark".into(), "camera".into(), i.to_string()],
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
                // Slight penalty for keyword-only match
                return Some((idx, cmd, score - 10));
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
            .insert_resource(registry)
            .add_systems(Update, handle_palette_toggle)
            .add_systems(EguiPrimaryContextPass, draw_command_palette);
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
    registry: Res<CommandRegistry>,
    mut spawn_events: MessageWriter<SpawnPrimitiveEvent>,
    mut set_mark_events: MessageWriter<SetCameraMarkEvent>,
    mut jump_mark_events: MessageWriter<JumpToMarkEvent>,
    mut jump_last_events: MessageWriter<JumpToLastPositionEvent>,
    mut save_events: MessageWriter<SaveSceneEvent>,
    mut load_events: MessageWriter<LoadSceneEvent>,
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
        }
    }

    if should_close {
        state.open = false;
    }

    Ok(())
}
