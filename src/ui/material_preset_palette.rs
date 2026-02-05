use bevy::prelude::*;
use bevy_editor_game::{BaseMaterialProps, MaterialDefinition, MaterialLibrary, MaterialRef};
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use super::fuzzy_palette::{fuzzy_filter, PaletteItem, PaletteState};
use super::material_preview::PresetPreviewState;
use super::theme::colors;
use crate::editor::{EditorMode, EditorState};
use crate::materials::{apply_material_def_standalone, remove_all_material_components};
use crate::selection::Selected;
use crate::ui::material_editor::EditingPreset;
use crate::utils::should_process_input;

/// Sentinel name used internally for the "new preset" action item.
const NEW_PRESET_SENTINEL: &str = "\x00__new_preset__";

/// Resource tracking state of the material preset search palette.
#[derive(Resource)]
pub struct MaterialPresetPaletteState {
    pub open: bool,
    pub palette_state: PaletteState,
    /// Name of the previously previewed preset, for change detection.
    prev_previewed_name: Option<String>,
    /// Previous query string, used to detect query changes.
    prev_query: String,
}

impl Default for MaterialPresetPaletteState {
    fn default() -> Self {
        Self {
            open: false,
            palette_state: PaletteState::default(),
            prev_previewed_name: None,
            prev_query: String::new(),
        }
    }
}

/// A library preset entry for fuzzy filtering.
struct PresetItem {
    name: String,
}

impl PaletteItem for PresetItem {
    fn label(&self) -> &str {
        &self.name
    }
}

pub struct MaterialPresetPalettePlugin;

impl Plugin for MaterialPresetPalettePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaterialPresetPaletteState>()
            .add_systems(Update, handle_preset_palette_toggle)
            .add_systems(EguiPrimaryContextPass, draw_material_preset_palette);
    }
}

/// Open the preset palette with F in Material mode.
fn handle_preset_palette_toggle(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut state: ResMut<MaterialPresetPaletteState>,
    editor_mode: Res<State<EditorMode>>,
    editor_state: Res<EditorState>,
    mut contexts: EguiContexts,
) {
    if state.open {
        return;
    }
    if !should_process_input(&editor_state, &mut contexts) {
        return;
    }
    if *editor_mode.get() != EditorMode::Material {
        return;
    }
    if keyboard.just_pressed(KeyCode::KeyF) {
        state.open = true;
        state.palette_state.reset();
        // Start on first library item (index 1), not "New Preset" (index 0)
        state.palette_state.selected_index = 1;
        state.prev_previewed_name = None;
    }
}

/// Draw the two-column material preset palette.
fn draw_material_preset_palette(
    mut contexts: EguiContexts,
    mut state: ResMut<MaterialPresetPaletteState>,
    mut preview_state: ResMut<PresetPreviewState>,
    editor_state: Res<EditorState>,
    editor_mode: Res<State<EditorMode>>,
    library: Res<MaterialLibrary>,
    selected_entities: Query<Entity, With<Selected>>,
    mut editing_preset: ResMut<EditingPreset>,
    mut commands: Commands,
) -> Result {
    if !editor_state.ui_enabled || !state.open {
        return Ok(());
    }

    // Close if mode changed away from Material
    if *editor_mode.get() != EditorMode::Material {
        state.open = false;
        preview_state.current_def = None;
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    // Build sorted list of library presets
    let mut names: Vec<String> = library.materials.keys().cloned().collect();
    names.sort();
    let items: Vec<PresetItem> = names.iter().map(|n| PresetItem { name: n.clone() }).collect();

    // Filter library presets
    let filtered = fuzzy_filter(&items, &state.palette_state.query);

    // Total item count: 1 (new preset) + filtered library items
    let total_count = 1 + filtered.len();

    // When the query changes, auto-select the first library match (or "New Preset" if none)
    if state.palette_state.query != state.prev_query {
        state.prev_query = state.palette_state.query.clone();
        state.palette_state.selected_index = if filtered.is_empty() { 0 } else { 1 };
    }

    // Clamp selected index
    state.palette_state.selected_index =
        state.palette_state.selected_index.min(total_count.saturating_sub(1));

    // Keyboard input
    let enter_pressed = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let down_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
    let up_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));

    if escape_pressed {
        state.open = false;
        preview_state.current_def = None;
        return Ok(());
    }

    // Handle Enter
    let has_selection = selected_entities.iter().next().is_some();
    if enter_pressed {
        if state.palette_state.selected_index == 0 {
            // "New Preset" selected — create and add to library
            let query = state.palette_state.query.trim().to_string();
            let new_name = new_preset_name(&query, &library);
            let def = MaterialDefinition {
                base: BaseMaterialProps::default(),
                extension: None,
            };
            let edit_name = new_name.clone();
            let entities: Vec<Entity> = selected_entities.iter().collect();
            commands.queue(move |world: &mut World| {
                world
                    .resource_mut::<MaterialLibrary>()
                    .materials
                    .insert(new_name.clone(), def.clone());
                for entity in &entities {
                    remove_all_material_components(world, *entity);
                    if let Ok(mut e) = world.get_entity_mut(*entity) {
                        e.insert(MaterialRef::Library(new_name.clone()));
                    }
                    apply_material_def_standalone(world, *entity, &def);
                }
            });
            // Always open for editing in the material panel
            editing_preset.0 = Some(edit_name);
            state.open = false;
            preview_state.current_def = None;
            return Ok(());
        } else if let Some(fi) = filtered.get(state.palette_state.selected_index - 1) {
            let preset_name = fi.item.name.clone();
            if let Some(def) = library.materials.get(&preset_name) {
                let def = def.clone();
                let entities: Vec<Entity> = selected_entities.iter().collect();
                let apply_name = preset_name.clone();
                commands.queue(move |world: &mut World| {
                    for entity in &entities {
                        remove_all_material_components(world, *entity);
                        if let Ok(mut e) = world.get_entity_mut(*entity) {
                            e.insert(MaterialRef::Library(apply_name.clone()));
                        }
                        apply_material_def_standalone(world, *entity, &def);
                    }
                });
            }
            // When no entity selected, open preset for editing in material panel
            if !has_selection {
                editing_preset.0 = Some(preset_name);
            }
            state.open = false;
            preview_state.current_def = None;
            return Ok(());
        }
    }

    if down_pressed && total_count > 0 {
        state.palette_state.selected_index =
            (state.palette_state.selected_index + 1).min(total_count - 1);
    }
    if up_pressed {
        state.palette_state.selected_index = state.palette_state.selected_index.saturating_sub(1);
    }

    // Update preview when highlighted library item changes (no preview for "New Preset")
    let current_name = if state.palette_state.selected_index == 0 {
        Some(NEW_PRESET_SENTINEL.to_string())
    } else {
        filtered
            .get(state.palette_state.selected_index - 1)
            .map(|fi| fi.item.name.clone())
    };
    if current_name != state.prev_previewed_name {
        state.prev_previewed_name = current_name.clone();
        preview_state.current_def = current_name
            .as_ref()
            .and_then(|n| library.materials.get(n))
            .cloned();
    }

    let preview_texture_id = preview_state.egui_texture_id;

    // Draw window
    let mut click_result: Option<String> = None;
    egui::Window::new("material_preset_palette")
        .collapsible(false)
        .resizable(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([580.0, 340.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            // -- Full-width header: mode indicator + search --
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("PRESETS")
                        .small()
                        .strong()
                        .color(colors::ACCENT_PURPLE),
                );
                ui.label(
                    egui::RichText::new("- Material library")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
            });
            ui.add_space(4.0);

            let response = ui.add(
                egui::TextEdit::singleline(&mut state.palette_state.query)
                    .hint_text("Type to search presets...")
                    .desired_width(f32::INFINITY),
            );
            if state.palette_state.just_opened {
                response.request_focus();
                state.palette_state.just_opened = false;
            }

            ui.separator();

            // -- Two-column middle: list (left) + preview (right) --
            let footer_reserve = 28.0;
            let middle_height = (ui.available_height() - footer_reserve).max(0.0);
            let middle_width = ui.available_width();
            let right_width = 230.0;
            let sep_width = 8.0;
            let left_width = (middle_width - right_width - sep_width).max(0.0);

            ui.allocate_ui(egui::vec2(middle_width, middle_height), |ui| {
                ui.horizontal_top(|ui| {
                    // Left: scrollable item list
                    ui.allocate_ui_with_layout(
                        egui::vec2(left_width, middle_height),
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                        egui::ScrollArea::vertical()
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                ui.set_min_width(left_width);

                                // "New Preset" action item (always index 0)
                                let new_selected = state.palette_state.selected_index == 0;
                                let new_label = if state.palette_state.query.trim().is_empty() {
                                    "+ New Preset".to_string()
                                } else {
                                    format!("+ New Preset \"{}\"", state.palette_state.query.trim())
                                };
                                let new_color = if new_selected {
                                    colors::ACCENT_GREEN
                                } else {
                                    colors::ACCENT_GREEN
                                };
                                let new_response = ui.selectable_label(
                                    new_selected,
                                    egui::RichText::new(&new_label).color(new_color),
                                );
                                if new_response.clicked() {
                                    click_result = Some(NEW_PRESET_SENTINEL.to_string());
                                }
                                if new_selected {
                                    new_response.scroll_to_me(Some(egui::Align::Center));
                                }

                                // Filtered library items (offset by 1)
                                if filtered.is_empty() && !state.palette_state.query.is_empty() {
                                    ui.label(
                                        egui::RichText::new("No matching presets")
                                            .color(colors::TEXT_MUTED)
                                            .italics(),
                                    );
                                }
                                for (display_idx, fi) in filtered.iter().enumerate() {
                                    let list_idx = display_idx + 1;
                                    let is_selected =
                                        list_idx == state.palette_state.selected_index;
                                    let text_color = if is_selected {
                                        colors::TEXT_PRIMARY
                                    } else {
                                        colors::TEXT_SECONDARY
                                    };

                                    let response = ui.selectable_label(
                                        is_selected,
                                        egui::RichText::new(fi.item.label())
                                            .color(text_color),
                                    );

                                    if response.clicked() {
                                        click_result = Some(fi.item.name.clone());
                                    }

                                    if is_selected {
                                        response.scroll_to_me(Some(egui::Align::Center));
                                    }
                                }
                            });
                    },
                    );

                    ui.separator();

                    // Right: preview
                    ui.vertical(|ui| {
                        ui.label(
                            egui::RichText::new("Preview")
                                .small()
                                .strong()
                                .color(colors::TEXT_SECONDARY),
                        );
                        ui.add_space(4.0);

                        if let Some(tex_id) = preview_texture_id {
                            let size = ui.available_width().min(220.0);
                            ui.image(egui::load::SizedTexture::new(tex_id, [size, size]));
                        } else {
                            ui.label(
                                egui::RichText::new("Preview loading...")
                                    .color(colors::TEXT_MUTED)
                                    .italics(),
                            );
                        }

                        if let Some(ref name) = state.prev_previewed_name {
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(name)
                                    .color(colors::TEXT_PRIMARY)
                                    .strong(),
                            );
                        }
                    });
                });
            });

            // -- Full-width footer --
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Enter")
                        .small()
                        .strong()
                        .color(colors::ACCENT_BLUE),
                );
                ui.label(
                    egui::RichText::new("to apply")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
                ui.add_space(10.0);
                ui.label(
                    egui::RichText::new("Esc")
                        .small()
                        .strong()
                        .color(colors::ACCENT_BLUE),
                );
                ui.label(
                    egui::RichText::new("to close")
                        .small()
                        .color(colors::TEXT_MUTED),
                );
            });
        });

    // Handle click selection
    if let Some(clicked_name) = click_result {
        if clicked_name == NEW_PRESET_SENTINEL {
            // Create new preset
            let query = state.palette_state.query.trim().to_string();
            let new_name = new_preset_name(&query, &library);
            let def = MaterialDefinition {
                base: BaseMaterialProps::default(),
                extension: None,
            };
            let edit_name = new_name.clone();
            let entities: Vec<Entity> = selected_entities.iter().collect();
            commands.queue(move |world: &mut World| {
                world
                    .resource_mut::<MaterialLibrary>()
                    .materials
                    .insert(new_name.clone(), def.clone());
                for entity in &entities {
                    remove_all_material_components(world, *entity);
                    if let Ok(mut e) = world.get_entity_mut(*entity) {
                        e.insert(MaterialRef::Library(new_name.clone()));
                    }
                    apply_material_def_standalone(world, *entity, &def);
                }
            });
            editing_preset.0 = Some(edit_name);
        } else if let Some(def) = library.materials.get(&clicked_name) {
            let def = def.clone();
            let preset_name = clicked_name.clone();
            let entities: Vec<Entity> = selected_entities.iter().collect();
            commands.queue(move |world: &mut World| {
                for entity in &entities {
                    remove_all_material_components(world, *entity);
                    if let Ok(mut e) = world.get_entity_mut(*entity) {
                        e.insert(MaterialRef::Library(preset_name.clone()));
                    }
                    apply_material_def_standalone(world, *entity, &def);
                }
            });
            if !has_selection {
                editing_preset.0 = Some(clicked_name);
            }
        }
        state.open = false;
        preview_state.current_def = None;
    }

    Ok(())
}

/// Generate a unique preset name. Uses the query if non-empty, otherwise "New Material".
/// Appends a number suffix if the name already exists.
fn new_preset_name(query: &str, library: &MaterialLibrary) -> String {
    let base = if query.is_empty() {
        "New Material".to_string()
    } else {
        query.to_string()
    };

    if !library.materials.contains_key(&base) {
        return base;
    }

    for i in 2.. {
        let candidate = format!("{} {}", base, i);
        if !library.materials.contains_key(&candidate) {
            return candidate;
        }
    }
    base
}
