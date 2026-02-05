//! Find object palette — search scene objects by name.

use bevy::prelude::*;
use bevy_egui::egui;

use crate::scene::SceneEntity;
use crate::selection::Selected;
use crate::ui::fuzzy_palette::{draw_fuzzy_palette, PaletteConfig, PaletteItem, PaletteResult, PaletteState};
use crate::ui::theme::colors;

use super::CommandPaletteState;

/// Entry for a scene object that implements PaletteItem
struct ObjectEntry {
    entity: Entity,
    name: String,
}

impl PaletteItem for ObjectEntry {
    fn label(&self) -> &str {
        &self.name
    }
}

/// Draw the find object palette
pub(super) fn draw_find_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    commands: &mut Commands,
    scene_objects: &Query<(Entity, &Name), With<SceneEntity>>,
    selected_entities: &Query<Entity, With<Selected>>,
) -> Result {
    // Build list of scene objects
    let objects: Vec<ObjectEntry> = scene_objects
        .iter()
        .map(|(entity, name)| ObjectEntry {
            entity,
            name: name.as_str().to_string(),
        })
        .collect();

    // Handle empty scene
    if objects.is_empty() {
        egui::Window::new("Find Object")
            .collapsible(false)
            .resizable(false)
            .title_bar(false)
            .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .fixed_size([400.0, 100.0])
            .show(ctx, |ui| {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("No objects in scene")
                            .color(colors::TEXT_MUTED)
                            .italics(),
                    );
                });
                ui.add_space(20.0);
                ui.separator();
                ui.horizontal(|ui| {
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

        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            state.open = false;
        }
        return Ok(());
    }

    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState {
        query: std::mem::take(&mut state.query),
        selected_index: state.selected_index,
        just_opened: state.just_opened,
    };

    let config = PaletteConfig {
        title: "FIND OBJECT",
        title_color: colors::ACCENT_CYAN,
        subtitle: "Search scene objects",
        hint_text: "Type to search...",
        action_label: "select",
        size: [400.0, 300.0],
        show_categories: false,
        preview_panel: None,
        ..Default::default()
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &objects, config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            if let Some(obj) = objects.get(index) {
                // Deselect all currently selected
                for selected in selected_entities.iter() {
                    commands.entity(selected).remove::<Selected>();
                }
                // Select the new entity
                commands.entity(obj.entity).insert(Selected);
            }
            state.open = false;
        }
        PaletteResult::Closed => {
            state.open = false;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}
