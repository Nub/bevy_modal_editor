//! Command registry, actions, filtering, and execution logic.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;

use bevy_spline_3d::prelude::SplineType;

use bevy_editor_game::{CustomEntityRegistry, PauseEvent, PlayEvent, ResetEvent};

use crate::commands::{RedoEvent, UndoEvent};
use crate::editor::{
    CameraMarks, CycleShadingModeEvent, EditorState, JumpToLastPositionEvent,
    JumpToMarkEvent, SetCameraMarkEvent, SetShadingModeEvent, StartInsertEvent, ToggleGridEvent,
    TogglePhysicsDebugEvent, TogglePhysicsEvent, ViewportShadingMode,
};
use crate::scene::{
    PrimitiveShape, SceneFile, SpawnDemoSceneEvent, SpawnEntityEvent, SpawnEntityKind,
    UnparentSelectedEvent,
    generators::{GenerateSceneEvent, SceneGenerator},
};
use crate::selection::Selected;
use crate::ui::theme::colors;
use crate::ui::SettingsWindowState;

use super::{
    CommandPaletteState, CustomMarkDialogState, HelpWindowState, PaletteMode,
    RenameSceneDialog,
};

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
    /// Spawn parametric stairs
    SpawnStairs,
    /// Spawn parametric ramp
    SpawnRamp,
    /// Spawn parametric arch
    SpawnArch,
    /// Spawn parametric L-shape
    SpawnLShape,
    /// Spawn a particle effect (bevy_hanabi)
    SpawnParticleEffect,
    /// Spawn a particle effect from a named preset
    SpawnParticlePreset(String),
    /// Spawn an effect from a named preset
    SpawnEffectPreset(String),
    /// Spawn a clustered decal
    SpawnDecal,
    /// Spawn a custom entity type registered by the game
    SpawnCustomEntity(String),
    /// Start simulation (play or resume)
    Play,
    /// Pause simulation
    Pause,
    /// Reset simulation to pre-play state
    Reset,
    /// Generate a museum scene showcasing all primitives and assets
    GenerateMuseum,
    /// Set a specific viewport shading mode
    SetShadingMode(ViewportShadingMode),
    /// Cycle to the next shading mode
    CycleShadingMode,
    /// Rename the current scene file
    RenameScene,
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

        // Blockout shapes (insertable)
        self.commands.push(Command {
            name: "Add Stairs".to_string(),
            keywords: vec!["steps".into(), "staircase".into(), "blockout".into()],
            category: "Blockout",
            action: CommandAction::SpawnStairs,
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add Ramp".to_string(),
            keywords: vec!["wedge".into(), "slope".into(), "incline".into(), "blockout".into()],
            category: "Blockout",
            action: CommandAction::SpawnRamp,
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add Arch".to_string(),
            keywords: vec!["doorway".into(), "door".into(), "opening".into(), "blockout".into()],
            category: "Blockout",
            action: CommandAction::SpawnArch,
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add L-Shape".to_string(),
            keywords: vec!["corner".into(), "wall".into(), "blockout".into()],
            category: "Blockout",
            action: CommandAction::SpawnLShape,
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

        // Effects (insertable)
        self.commands.push(Command {
            name: "Add Fog Volume".to_string(),
            keywords: vec!["volumetric".into(), "fog".into(), "atmosphere".into(), "mist".into(), "haze".into()],
            category: "Effects",
            action: CommandAction::SpawnFogVolume,
            insertable: true,
        });
        self.commands.push(Command {
            name: "Add Particle Effect".to_string(),
            keywords: vec!["particle".into(), "emitter".into(), "vfx".into(), "fx".into(), "hanabi".into(), "fire".into(), "smoke".into(), "sparks".into()],
            category: "Effects",
            action: CommandAction::SpawnParticleEffect,
            insertable: true,
        });

        // Decals (insertable)
        self.commands.push(Command {
            name: "Add Decal".to_string(),
            keywords: vec!["decal".into(), "sticker".into(), "projected".into(), "texture".into(), "splat".into()],
            category: "Effects",
            action: CommandAction::SpawnDecal,
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
            name: "Rename Scene".to_string(),
            keywords: vec!["rename".into(), "file".into(), "name".into()],
            category: "Scene",
            action: CommandAction::RenameScene,
            insertable: false,
        });
        self.commands.push(Command {
            name: "Spawn Demo Scene".to_string(),
            keywords: vec!["example".into(), "sample".into(), "test".into(), "create".into()],
            category: "Scene",
            action: CommandAction::SpawnDemoScene,
            insertable: false,
        });
        self.commands.push(Command {
            name: "Generate Museum".to_string(),
            keywords: vec!["museum".into(), "gallery".into(), "showcase".into(), "assets".into(), "browse".into()],
            category: "Scene",
            action: CommandAction::GenerateMuseum,
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

        // Viewport shading modes
        self.commands.push(Command {
            name: "Shading: Rendered".to_string(),
            keywords: vec!["viewport".into(), "full".into(), "materials".into(), "lighting".into()],
            category: "View",
            action: CommandAction::SetShadingMode(ViewportShadingMode::Rendered),
            insertable: false,
        });
        self.commands.push(Command {
            name: "Shading: Solid".to_string(),
            keywords: vec!["viewport".into(), "flat".into(), "color".into(), "no".into(), "texture".into()],
            category: "View",
            action: CommandAction::SetShadingMode(ViewportShadingMode::Solid),
            insertable: false,
        });
        self.commands.push(Command {
            name: "Shading: Wireframe".to_string(),
            keywords: vec!["viewport".into(), "edges".into(), "lines".into(), "mesh".into()],
            category: "View",
            action: CommandAction::SetShadingMode(ViewportShadingMode::Wireframe),
            insertable: false,
        });
        self.commands.push(Command {
            name: "Shading: Unlit".to_string(),
            keywords: vec!["viewport".into(), "flat".into(), "no".into(), "shadows".into(), "fullbright".into()],
            category: "View",
            action: CommandAction::SetShadingMode(ViewportShadingMode::Unlit),
            insertable: false,
        });
        self.commands.push(Command {
            name: "Cycle Shading Mode".to_string(),
            keywords: vec!["viewport".into(), "next".into(), "switch".into()],
            category: "View",
            action: CommandAction::CycleShadingMode,
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

        // Game
        self.commands.push(Command {
            name: "Play".to_string(),
            keywords: vec!["start".into(), "run".into(), "simulate".into(), "f5".into()],
            category: "Game",
            action: CommandAction::Play,
            insertable: false,
        });
        self.commands.push(Command {
            name: "Pause".to_string(),
            keywords: vec!["stop".into(), "freeze".into(), "f6".into()],
            category: "Game",
            action: CommandAction::Pause,
            insertable: false,
        });
        self.commands.push(Command {
            name: "Reset".to_string(),
            keywords: vec!["restore".into(), "revert".into(), "f7".into()],
            category: "Game",
            action: CommandAction::Reset,
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

/// Register custom entity types from the game into the command palette
pub(super) fn register_custom_entity_commands(
    custom_entities: Res<CustomEntityRegistry>,
    mut registry: ResMut<CommandRegistry>,
) {
    for entry in &custom_entities.entries {
        registry.commands.push(Command {
            name: format!("Add {}", entry.entity_type.name),
            keywords: entry
                .entity_type
                .keywords
                .iter()
                .map(|k| k.to_string())
                .collect(),
            category: entry.entity_type.category,
            action: CommandAction::SpawnCustomEntity(entry.entity_type.name.to_string()),
            insertable: true,
        });
    }
}

/// Get filtered and sorted commands based on query using skim fuzzy matcher.
///
/// - `insert_only`: if true, only show commands marked `insertable`
/// - `exclude_insertable`: if true, hide commands marked `insertable`
pub fn filter_commands<'a>(
    commands: &'a [Command],
    query: &str,
    insert_only: bool,
    exclude_insertable: bool,
) -> Vec<(usize, &'a Command, i64)> {
    let matcher = SkimMatcherV2::default();

    // Filter by mode
    let mode_filtered: Vec<_> = commands
        .iter()
        .enumerate()
        .filter(|(_, cmd)| {
            if insert_only && !cmd.insertable {
                return false;
            }
            if exclude_insertable && cmd.insertable {
                return false;
            }
            true
        })
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

/// System parameter grouping all command palette event writers
#[derive(SystemParam)]
pub(super) struct CommandEvents<'w> {
    pub spawn_entity: MessageWriter<'w, SpawnEntityEvent>,
    pub unparent: MessageWriter<'w, UnparentSelectedEvent>,
    pub set_mark: MessageWriter<'w, SetCameraMarkEvent>,
    pub jump_mark: MessageWriter<'w, JumpToMarkEvent>,
    pub jump_last: MessageWriter<'w, JumpToLastPositionEvent>,
    pub toggle_debug: MessageWriter<'w, TogglePhysicsDebugEvent>,
    pub toggle_physics: MessageWriter<'w, TogglePhysicsEvent>,
    pub toggle_grid: MessageWriter<'w, ToggleGridEvent>,
    pub start_insert: MessageWriter<'w, StartInsertEvent>,
    pub spawn_demo: MessageWriter<'w, SpawnDemoSceneEvent>,
    pub undo: MessageWriter<'w, UndoEvent>,
    pub redo: MessageWriter<'w, RedoEvent>,
    pub play: MessageWriter<'w, PlayEvent>,
    pub pause: MessageWriter<'w, PauseEvent>,
    pub reset: MessageWriter<'w, ResetEvent>,
    pub generate_scene: MessageWriter<'w, GenerateSceneEvent>,
    pub set_shading: MessageWriter<'w, SetShadingModeEvent>,
    pub cycle_shading: MessageWriter<'w, CycleShadingModeEvent>,
}

/// System parameter grouping palette UI state resources
#[derive(SystemParam)]
pub(super) struct PaletteState2<'w> {
    pub help_state: ResMut<'w, HelpWindowState>,
    pub settings_state: ResMut<'w, SettingsWindowState>,
    pub custom_mark_state: ResMut<'w, CustomMarkDialogState>,
    pub rename_dialog: ResMut<'w, RenameSceneDialog>,
    pub component_editor_state: ResMut<'w, super::super::inspector::ComponentEditorState>,
    pub component_registry: ResMut<'w, super::components::ComponentRegistry>,
    pub removable_cache: Res<'w, super::RemovableComponentsCache>,
}

/// Draw the command palette (Commands mode)
pub(super) fn draw_commands_palette(
    ctx: &bevy_egui::egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    palette_state2: &mut PaletteState2,
    editor_state: &mut ResMut<EditorState>,
    scene_file: &Res<SceneFile>,
    registry: &Res<CommandRegistry>,
    custom_registry: &Res<CustomEntityRegistry>,
    selected: &Query<Entity, With<Selected>>,
    events: &mut CommandEvents,
    commands: &mut Commands,
) -> Result {
    use bevy_egui::egui;

    // Commands mode excludes insertable items (they live in Insert mode now)
    let filtered = filter_commands(&registry.commands, &state.query, false, true);

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
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([400.0, 300.0])
        .show(ctx, |ui| {
            // Search input
            let response = ui.add(
                egui::TextEdit::singleline(&mut state.query)
                    .hint_text("Type to search commands...")
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
        execute_command(
            action,
            state,
            palette_state2,
            editor_state,
            scene_file,
            custom_registry,
            selected,
            events,
            commands,
        );
    }

    // Only close if the command didn't switch to another palette mode
    // (e.g. InsertGltf opens AssetBrowser, SaveScene opens AssetBrowser, etc.)
    if should_close && state.mode == PaletteMode::Commands {
        state.open = false;
    }

    Ok(())
}

/// Execute a command action
fn execute_command(
    action: CommandAction,
    state: &mut ResMut<CommandPaletteState>,
    palette_state2: &mut PaletteState2,
    editor_state: &mut ResMut<EditorState>,
    scene_file: &Res<SceneFile>,
    custom_registry: &Res<CustomEntityRegistry>,
    selected: &Query<Entity, With<Selected>>,
    events: &mut CommandEvents,
    commands: &mut Commands,
) {
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
        CommandAction::SpawnStairs => {
            events.spawn_entity.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Stairs,
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            });
        }
        CommandAction::SpawnRamp => {
            events.spawn_entity.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Ramp,
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            });
        }
        CommandAction::SpawnArch => {
            events.spawn_entity.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Arch,
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            });
        }
        CommandAction::SpawnLShape => {
            events.spawn_entity.write(SpawnEntityEvent {
                kind: SpawnEntityKind::LShape,
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            });
        }
        CommandAction::SpawnParticleEffect => {
            events.spawn_entity.write(SpawnEntityEvent {
                kind: SpawnEntityKind::ParticleEffect,
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            });
        }
        CommandAction::SpawnParticlePreset(ref preset_name) => {
            events.spawn_entity.write(SpawnEntityEvent {
                kind: SpawnEntityKind::ParticlePreset(preset_name.clone()),
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            });
        }
        CommandAction::SpawnEffectPreset(ref preset_name) => {
            events.spawn_entity.write(SpawnEntityEvent {
                kind: SpawnEntityKind::EffectPreset(preset_name.clone()),
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            });
        }
        CommandAction::SpawnDecal => {
            events.spawn_entity.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Decal,
                position: Vec3::ZERO,
                rotation: Quat::IDENTITY,
            });
        }
        CommandAction::SpawnCustomEntity(ref type_name) => {
            let position = custom_registry
                .entries
                .iter()
                .find(|e| e.entity_type.name == type_name)
                .map(|e| e.entity_type.default_position)
                .unwrap_or(Vec3::ZERO);
            events.spawn_entity.write(SpawnEntityEvent {
                kind: SpawnEntityKind::Custom(type_name.clone()),
                position,
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
            state.open_save_scene(scene_file.path.as_deref());
        }
        CommandAction::LoadScene => {
            state.open_load_scene();
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
            }
        }
        CommandAction::InsertGltf => {
            state.open_asset_browser_insert_gltf();
        }
        CommandAction::InsertScene => {
            state.open_asset_browser_insert_scene();
        }
        CommandAction::Play => {
            events.play.write(PlayEvent);
        }
        CommandAction::Pause => {
            events.pause.write(PauseEvent);
        }
        CommandAction::Reset => {
            events.reset.write(ResetEvent);
        }
        CommandAction::GenerateMuseum => {
            events.generate_scene.write(GenerateSceneEvent {
                generator: SceneGenerator::Museum,
            });
        }
        CommandAction::SetShadingMode(mode) => {
            events.set_shading.write(SetShadingModeEvent(mode));
        }
        CommandAction::CycleShadingMode => {
            events.cycle_shading.write(CycleShadingModeEvent);
        }
        CommandAction::RenameScene => {
            if let Some(path) = scene_file.path.as_ref() {
                let current_name = std::path::Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("")
                    .trim_end_matches(".scn.ron")
                    .trim_end_matches(".ron")
                    .to_string();
                palette_state2.rename_dialog.open = true;
                palette_state2.rename_dialog.name = current_name;
                palette_state2.rename_dialog.just_opened = true;
            }
        }
    }
}

