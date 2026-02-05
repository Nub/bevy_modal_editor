//! Material preset palette — browse and apply material library presets.

use bevy::prelude::*;
use bevy_editor_game::{BaseMaterialProps, MaterialDefinition, MaterialLibrary, MaterialRef};
use bevy_egui::egui;

use crate::materials::{apply_material_def_standalone, remove_all_material_components};
use crate::selection::Selected;
use crate::ui::fuzzy_palette::{
    draw_fuzzy_palette, fuzzy_filter, PaletteConfig, PaletteItem, PaletteResult, PaletteState,
};
use crate::ui::material_editor::EditingPreset;
use crate::ui::material_preview::PresetPreviewState;
use crate::ui::theme::colors;

use super::CommandPaletteState;

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

/// Draw the material preset palette using the shared fuzzy palette widget.
pub(super) fn draw_material_preset_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    preview_state: &mut ResMut<PresetPreviewState>,
    library: &Res<MaterialLibrary>,
    selected_entities: &Query<Entity, With<Selected>>,
    editing_preset: &mut ResMut<EditingPreset>,
    commands: &mut Commands,
) -> Result {
    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState {
        query: std::mem::take(&mut state.query),
        selected_index: state.selected_index,
        just_opened: state.just_opened,
    };

    // Build items: "New Preset" (always-visible) at index 0, then sorted library presets
    let new_label = if palette_state.query.trim().is_empty() {
        "+ New Preset".to_string()
    } else {
        format!("+ New Preset \"{}\"", palette_state.query.trim())
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
    if palette_state.query != state.prev_query {
        state.prev_query = palette_state.query.clone();
        let filtered = fuzzy_filter(&items, &palette_state.query);
        // Find first non-pinned item's display position
        let first_library = filtered
            .iter()
            .position(|fi| !fi.item.is_new_preset)
            .unwrap_or(0);
        palette_state.selected_index = first_library;
    }

    // Update preview based on currently highlighted item
    {
        let filtered = fuzzy_filter(&items, &palette_state.query);
        let clamped = if filtered.is_empty() {
            0
        } else {
            palette_state.selected_index.min(filtered.len() - 1)
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

    let preview_texture_id = preview_state.texture.egui_texture_id;
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
        ..Default::default()
    };

    let has_selection = selected_entities.iter().next().is_some();

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &items, config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            if items[index].is_new_preset {
                // "New Preset" selected — create and add to library
                let query = state.query.trim().to_string();
                let new_name = new_preset_name(&query, library);
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
pub(super) fn new_preset_name(query: &str, library: &MaterialLibrary) -> String {
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
