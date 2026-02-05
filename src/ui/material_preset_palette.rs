use bevy::prelude::*;
use bevy_editor_game::{BaseMaterialProps, MaterialDefinition, MaterialLibrary, MaterialRef};
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use super::fuzzy_palette::{
    draw_fuzzy_palette, fuzzy_filter, PaletteConfig, PaletteItem, PaletteResult, PaletteState,
};
use super::material_preview::PresetPreviewState;
use super::theme::colors;
use crate::editor::{EditorMode, EditorState};
use crate::materials::{apply_material_def_standalone, remove_all_material_components};
use crate::selection::Selected;
use crate::ui::material_editor::EditingPreset;
use crate::utils::should_process_input;

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
    is_new_preset: bool,
}

impl PaletteItem for PresetItem {
    fn label(&self) -> &str {
        &self.name
    }

    fn always_visible(&self) -> bool {
        self.is_new_preset
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

/// Draw the material preset palette using the shared fuzzy palette widget.
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

    // Build items: "New Preset" (always-visible) at index 0, then sorted library presets
    let new_label = if state.palette_state.query.trim().is_empty() {
        "+ New Preset".to_string()
    } else {
        format!("+ New Preset \"{}\"", state.palette_state.query.trim())
    };
    let mut items: Vec<PresetItem> = vec![PresetItem {
        name: new_label,
        is_new_preset: true,
    }];
    let mut names: Vec<String> = library.materials.keys().cloned().collect();
    names.sort();
    items.extend(names.iter().map(|n| PresetItem {
        name: n.clone(),
        is_new_preset: false,
    }));

    // When the query changes, auto-select the first library match (skip pinned "New Preset")
    if state.palette_state.query != state.prev_query {
        state.prev_query = state.palette_state.query.clone();
        let filtered = fuzzy_filter(&items, &state.palette_state.query);
        // Find first non-pinned item's display position
        let first_library = filtered
            .iter()
            .position(|fi| !fi.item.is_new_preset)
            .unwrap_or(0);
        state.palette_state.selected_index = first_library;
    }

    // Update preview based on currently highlighted item
    {
        let filtered = fuzzy_filter(&items, &state.palette_state.query);
        let clamped = if filtered.is_empty() {
            0
        } else {
            state.palette_state.selected_index.min(filtered.len() - 1)
        };
        let current_name = filtered.get(clamped).and_then(|fi| {
            if fi.item.is_new_preset {
                None
            } else {
                Some(fi.item.name.clone())
            }
        });
        if current_name != state.prev_previewed_name {
            state.prev_previewed_name = current_name.clone();
            preview_state.current_def = current_name
                .as_ref()
                .and_then(|n| library.materials.get(n))
                .cloned();
        }
    }

    let preview_texture_id = preview_state.egui_texture_id;
    let preview_name = state.prev_previewed_name.clone();

    let preview_panel: Box<dyn FnOnce(&mut egui::Ui) + '_> = Box::new(move |ui: &mut egui::Ui| {
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

        if let Some(ref name) = preview_name {
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(name)
                    .color(colors::TEXT_PRIMARY)
                    .strong(),
            );
        }
    });

    let config = PaletteConfig {
        title: "PRESETS",
        title_color: colors::ACCENT_PURPLE,
        subtitle: "Material library",
        hint_text: "Type to search presets...",
        action_label: "apply",
        size: [342.0, 340.0],
        show_categories: false,
        preview_panel: Some(preview_panel),
    };

    let has_selection = selected_entities.iter().next().is_some();

    match draw_fuzzy_palette(ctx, &mut state.palette_state, &items, config) {
        PaletteResult::Selected(index) => {
            if items[index].is_new_preset {
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
                editing_preset.0 = Some(edit_name);
            } else {
                let preset_name = items[index].name.clone();
                if let Some(def) = library.materials.get(&preset_name) {
                    let def = def.clone();
                    let apply_name = preset_name.clone();
                    let entities: Vec<Entity> = selected_entities.iter().collect();
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
                if !has_selection {
                    editing_preset.0 = Some(items[index].name.clone());
                }
            }
            state.open = false;
            preview_state.current_def = None;
        }
        PaletteResult::Closed => {
            state.open = false;
            preview_state.current_def = None;
        }
        PaletteResult::Open => {}
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
