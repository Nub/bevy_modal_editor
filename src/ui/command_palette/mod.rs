//! Unified command palette module.
//!
//! All palette-style popups (commands, insert, find object, entity picker,
//! material preset, asset browser, component search/add/remove) are handled
//! through a single `CommandPaletteState` resource and dispatched by
//! `PaletteMode`.

mod asset_browser;
pub(super) mod commands;
pub(super) mod components;
mod entity_picker;
mod find_object;
mod insert;
mod material_preset;

use std::any::TypeId;
use std::path::Path;

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use bevy_editor_game::{CustomEntityRegistry, MaterialLibrary};

use crate::editor::{
    CameraMarks, EditorMode, EditorState, InsertState, SetCameraMarkEvent,
};
use crate::scene::{LoadSceneEvent, SaveSceneEvent, SceneEntity, SceneFile};
use crate::selection::Selected;
use crate::ui::gltf_preview::GltfPreviewState;
use crate::ui::insert_preview::InsertPreviewState;
use crate::ui::material_editor::EditingPreset;
use crate::ui::material_preview::PresetPreviewState;
use crate::ui::theme::colors;
use crate::utils::should_process_input;

// Re-export public types from submodules
pub use asset_browser::{TexturePickData, TexturePickResult, TextureSlot};
pub use commands::{CommandAction, CommandRegistry};
pub use entity_picker::{
    CurrentInspectedEntity, EntityPickerSelection, PendingEntityPickerRequest,
    PendingEntitySelection, draw_entity_field, make_callback_id,
};

// ── PaletteMode ──────────────────────────────────────────────────────

/// The mode the command palette is operating in
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum PaletteMode {
    /// Normal command search
    #[default]
    Commands,
    /// Insert mode — only show insertable objects
    Insert,
    /// Component search — show components on selected entity
    ComponentSearch,
    /// Add component — show all available components to add
    AddComponent,
    /// Remove component — show components that can be removed
    RemoveComponent,
    /// Find object by name in the scene
    FindObject,
    /// Pick an entity for a field reference
    EntityPicker,
    /// Browse/apply material library presets
    MaterialPreset,
    /// Browse asset files (load/save scene, insert GLTF, pick texture)
    AssetBrowser,
}

// ── CommandPaletteState ──────────────────────────────────────────────

/// Resource to track command palette state (shared across all palette modes)
#[derive(Resource)]
pub struct CommandPaletteState {
    // ── Shared ──
    pub open: bool,
    pub query: String,
    pub selected_index: usize,
    pub just_opened: bool,
    pub mode: PaletteMode,
    pub target_entity: Option<Entity>,

    // ── EntityPicker ──
    pub picker_field_name: String,
    pub picker_callback_id: u64,

    // ── MaterialPreset ──
    pub(crate) prev_previewed_name: Option<String>,
    pub(crate) prev_query: String,

    // ── AssetBrowser ──
    pub(crate) browse_operation: Option<asset_browser::BrowseOperation>,
    pub(crate) asset_items: Vec<asset_browser::AssetFileItem>,
    pub(crate) preview_path: Option<String>,
    pub(crate) preview_handle: Option<Handle<Image>>,
    pub(crate) preview_texture_id: Option<egui::TextureId>,
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
            picker_field_name: String::new(),
            picker_callback_id: 0,
            prev_previewed_name: None,
            prev_query: String::new(),
            browse_operation: None,
            asset_items: Vec::new(),
            preview_path: None,
            preview_handle: None,
            preview_texture_id: None,
        }
    }
}

impl CommandPaletteState {
    /// Reset shared fields and open the palette in a specific mode
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

    /// Open the palette in FindObject mode
    pub fn open_find_object(&mut self) {
        self.open_mode(PaletteMode::FindObject);
    }

    /// Open the palette in EntityPicker mode
    pub fn open_entity_picker(&mut self, entity: Entity, field_name: &str, callback_id: u64) {
        self.open_mode(PaletteMode::EntityPicker);
        self.target_entity = Some(entity);
        self.picker_field_name = field_name.to_string();
        self.picker_callback_id = callback_id;
    }

    /// Open the palette in MaterialPreset mode
    pub fn open_material_preset(&mut self) {
        self.open_mode(PaletteMode::MaterialPreset);
        // Start on first library item (index 1), not "New Preset" (index 0)
        self.selected_index = 1;
        self.prev_previewed_name = None;
        self.prev_query.clear();
    }

    // ── AssetBrowser open helpers ──

    fn open_asset_browser(&mut self, operation: asset_browser::BrowseOperation, extensions: &[&str]) {
        self.asset_items = asset_browser::scan_assets(extensions);
        self.open_mode(PaletteMode::AssetBrowser);
        self.browse_operation = Some(operation);
        self.preview_path = None;
        self.preview_handle = None;
        self.preview_texture_id = None;
    }

    pub fn open_load_scene(&mut self) {
        self.open_asset_browser(asset_browser::BrowseOperation::LoadScene, &["ron"]);
    }

    pub fn open_save_scene(&mut self, current_path: Option<&str>) {
        self.open_asset_browser(asset_browser::BrowseOperation::SaveScene, &["ron"]);

        // Prepend virtual "Save as" item
        self.asset_items.insert(
            0,
            asset_browser::AssetFileItem {
                relative_path: String::new(),
                filename: "Save as new file...".to_string(),
                directory: String::new(),
                is_save_as: true,
            },
        );

        // Pre-populate the query with the current scene filename (without extension)
        if let Some(path) = current_path {
            let name = Path::new(path)
                .file_stem()
                .and_then(|n| n.to_str())
                .unwrap_or("");
            self.query = name.to_string();
        }
    }

    pub fn open_insert_gltf(&mut self) {
        self.open_asset_browser(asset_browser::BrowseOperation::InsertGltf, &["gltf", "glb"]);
    }

    pub fn open_insert_scene(&mut self) {
        self.open_asset_browser(asset_browser::BrowseOperation::InsertScene, &["ron"]);
    }

    pub fn open_pick_texture(&mut self, slot: TextureSlot, entity: Option<Entity>) {
        self.open_asset_browser(
            asset_browser::BrowseOperation::PickTexture { slot, entity },
            &["png", "jpg", "jpeg", "hdr", "exr", "tga", "bmp"],
        );
    }

    // ── Aliases for backward compat in commands module ──

    pub(crate) fn open_asset_browser_insert_gltf(&mut self) {
        self.open_insert_gltf();
    }

    pub(crate) fn open_asset_browser_insert_scene(&mut self) {
        self.open_insert_scene();
    }
}

/// Open the command palette in AddComponent mode for a specific entity
pub fn open_add_component_palette(state: &mut CommandPaletteState, entity: Entity) {
    state.open_add_component(entity);
}

// ── Auxiliary resources ──────────────────────────────────────────────

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

// ── Plugin ───────────────────────────────────────────────────────────

pub struct CommandPalettePlugin;

impl Plugin for CommandPalettePlugin {
    fn build(&self, app: &mut App) {
        let mut registry = CommandRegistry::default();
        registry.build_static_commands();

        app.init_resource::<CommandPaletteState>()
            .init_resource::<HelpWindowState>()
            .init_resource::<CustomMarkDialogState>()
            .init_resource::<RemovableComponentsCache>()
            .init_resource::<components::ComponentRegistry>()
            .init_resource::<PendingEntitySelection>()
            .init_resource::<CurrentInspectedEntity>()
            .init_resource::<PendingEntityPickerRequest>()
            .init_resource::<TexturePickResult>()
            .insert_resource(registry)
            .add_systems(PreStartup, commands::register_custom_entity_commands)
            .add_systems(Update, (handle_palette_toggle, components::populate_removable_components))
            .add_systems(
                EguiPrimaryContextPass,
                (draw_command_palette, draw_help_window, draw_custom_mark_dialog),
            );
    }
}

// ── Consolidated keyboard handler ────────────────────────────────────

/// Unified keyboard handler for all palette modes.
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

    let mode = *editor_mode.get();

    // "F" key — context-dependent
    if keyboard.just_pressed(KeyCode::KeyF) {
        if mode == EditorMode::Material {
            state.open_material_preset();
            return;
        }
        if mode != EditorMode::Hierarchy {
            state.open_find_object();
            return;
        }
    }

    // "/" key
    if keyboard.just_pressed(KeyCode::Slash) {
        if mode == EditorMode::Hierarchy {
            state.open_find_object();
            return;
        }
        if mode == EditorMode::ObjectInspector {
            state.open_component_search();
            return;
        }
    }

    // "X" key opens remove component palette in ObjectInspector mode
    if keyboard.just_pressed(KeyCode::KeyX) && mode == EditorMode::ObjectInspector {
        if let Some(entity) = selected.iter().next() {
            removable_cache.entity = None;
            removable_cache.components.clear();
            state.open_remove_component(entity);
        }
        return;
    }

    // "C" key opens command palette
    if keyboard.just_pressed(KeyCode::KeyC) {
        state.open_commands();
        registry.add_mark_commands(&marks);
    }
}

// ── Bundled system parameters for dispatch ───────────────────────────

/// Resources used by the asset browser mode
#[derive(SystemParam)]
struct AssetBrowserParams<'w> {
    load_events: MessageWriter<'w, LoadSceneEvent>,
    save_events: MessageWriter<'w, SaveSceneEvent>,
    insert_state: ResMut<'w, InsertState>,
    next_mode: ResMut<'w, NextState<EditorMode>>,
    texture_pick: ResMut<'w, TexturePickResult>,
    asset_server: Res<'w, AssetServer>,
    gltf_preview_state: ResMut<'w, GltfPreviewState>,
}

/// Resources used by modes other than AssetBrowser
#[derive(SystemParam)]
struct ModeParams<'w> {
    insert_preview_state: ResMut<'w, InsertPreviewState>,
    preview_state: ResMut<'w, PresetPreviewState>,
    pending_selection: ResMut<'w, PendingEntitySelection>,
    editing_preset: ResMut<'w, EditingPreset>,
    library: Res<'w, MaterialLibrary>,
    editor_mode: Res<'w, State<EditorMode>>,
    type_registry: Res<'w, AppTypeRegistry>,
    custom_registry: Res<'w, CustomEntityRegistry>,
}

// ── Main draw dispatch ───────────────────────────────────────────────

/// Draw the command palette, dispatching to the appropriate mode's draw function.
fn draw_command_palette(
    mut contexts: EguiContexts,
    mut state: ResMut<CommandPaletteState>,
    mut palette_state2: commands::PaletteState2,
    mut editor_state: ResMut<EditorState>,
    scene_file: Res<SceneFile>,
    registry: Res<CommandRegistry>,
    selected: Query<Entity, With<Selected>>,
    scene_objects: Query<(Entity, &Name), With<SceneEntity>>,
    mut ab: AssetBrowserParams,
    mut mp: ModeParams,
    mut events: commands::CommandEvents,
    mut bevy_commands: Commands,
) -> Result {
    if !editor_state.ui_enabled {
        return Ok(());
    }

    if !state.open {
        return Ok(());
    }

    match state.mode {
        PaletteMode::ComponentSearch => {
            let ctx = contexts.ctx_mut()?;
            return components::draw_component_search_palette(
                ctx,
                &mut state,
                &mut palette_state2.component_editor_state,
                &mp.type_registry,
                &selected,
            );
        }
        PaletteMode::AddComponent => {
            let ctx = contexts.ctx_mut()?;
            return components::draw_add_component_palette(
                ctx,
                &mut state,
                &mut palette_state2.component_registry,
                &mp.type_registry,
                &mut bevy_commands,
            );
        }
        PaletteMode::RemoveComponent => {
            let ctx = contexts.ctx_mut()?;
            return components::draw_remove_component_palette(
                ctx,
                &mut state,
                &palette_state2.removable_cache,
                &selected,
                &mut bevy_commands,
            );
        }
        PaletteMode::Insert => {
            let ctx = contexts.ctx_mut()?;
            return insert::draw_insert_palette(
                ctx,
                &mut state,
                &registry,
                &mut mp.insert_preview_state,
                &mut events,
                &mut ab.next_mode,
            );
        }
        PaletteMode::FindObject => {
            let ctx = contexts.ctx_mut()?;
            return find_object::draw_find_palette(
                ctx,
                &mut state,
                &mut bevy_commands,
                &scene_objects,
                &selected,
            );
        }
        PaletteMode::EntityPicker => {
            let ctx = contexts.ctx_mut()?;
            return entity_picker::draw_entity_picker(
                ctx,
                &mut state,
                &mut mp.pending_selection,
                &scene_objects,
            );
        }
        PaletteMode::MaterialPreset => {
            if *mp.editor_mode.get() != EditorMode::Material {
                state.open = false;
                mp.preview_state.current_def = None;
                return Ok(());
            }
            let ctx = contexts.ctx_mut()?;
            return material_preset::draw_material_preset_palette(
                ctx,
                &mut state,
                &mut mp.preview_state,
                &mp.library,
                &selected,
                &mut mp.editing_preset,
                &mut bevy_commands,
            );
        }
        PaletteMode::AssetBrowser => {
            return asset_browser::draw_asset_browser(
                &mut contexts,
                &mut state,
                &mut ab.load_events,
                &mut ab.save_events,
                &mut events.start_insert,
                &mut ab.insert_state,
                &mut ab.next_mode,
                &mut ab.texture_pick,
                &ab.asset_server,
                &mut ab.gltf_preview_state,
            );
        }
        PaletteMode::Commands => {
            let ctx = contexts.ctx_mut()?;
            return commands::draw_commands_palette(
                ctx,
                &mut state,
                &mut palette_state2,
                &mut editor_state,
                &mut mp.insert_preview_state,
                &scene_file,
                &registry,
                &mp.custom_registry,
                &selected,
                &mut events,
                &mut bevy_commands,
                &mut ab.next_mode,
            );
        }
    }
}

// ── Help window ──────────────────────────────────────────────────────

fn draw_help_window(
    mut contexts: EguiContexts,
    mut state: ResMut<HelpWindowState>,
    editor_state: Res<EditorState>,
) -> Result {
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
                ui.vertical(|ui| {
                    ui.set_min_width(220.0);

                    help_section(ui, "Mode Switching");
                    shortcut_row(ui, "E", "Edit mode");
                    shortcut_row(ui, "I", "Insert mode");
                    shortcut_row(ui, "O", "Object Inspector mode");
                    shortcut_row(ui, "H", "Hierarchy mode");
                    shortcut_row(ui, "B", "Blockout mode");
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
                    help_section(ui, "Selection & Edit");
                    shortcut_row(ui, "Click", "Select object");
                    shortcut_row(ui, "Shift+Click", "Multi-select");
                    shortcut_row(ui, "Ctrl+D", "Duplicate");
                    shortcut_row(ui, "Arrows", "Nudge selected");
                    shortcut_row(ui, "G", "Group selected");
                    shortcut_row(ui, "Delete/X", "Delete selected");
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(16.0);

                ui.vertical(|ui| {
                    ui.set_min_width(220.0);

                    help_section(ui, "Edit Mode - Transform");
                    shortcut_row(ui, "Q", "Translate");
                    shortcut_row(ui, "W", "Rotate");
                    shortcut_row(ui, "E", "Scale");
                    shortcut_row(ui, "R", "Place (raycast)");
                    shortcut_row(ui, "T", "Snap to object");
                    shortcut_row(ui, "A/S/D", "Constrain X/Y/Z");
                    shortcut_row(ui, "J/K", "Step -/+");
                    shortcut_row(ui, "Alt+Drag", "Edge snap");

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
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(16.0);

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
                    help_section(ui, "Blockout Mode (B)");
                    shortcut_row(ui, "1-5", "Select shape");
                    shortcut_row(ui, "WASDQE", "Select face");
                    shortcut_row(ui, "R", "Rotate 90°");
                    shortcut_row(ui, "Enter", "Place tile");
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

// ── Custom mark dialog ───────────────────────────────────────────────

fn draw_custom_mark_dialog(
    mut contexts: EguiContexts,
    mut state: ResMut<CustomMarkDialogState>,
    mut set_mark_events: MessageWriter<SetCameraMarkEvent>,
    editor_state: Res<EditorState>,
) -> Result {
    if !editor_state.ui_enabled {
        return Ok(());
    }

    if !state.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let mut should_close = false;
    let mut should_save = false;

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
