//! Insert mode palette with 3D preview panel.

use bevy::prelude::*;
use bevy_egui::egui;

use crate::editor::{EditorMode, InsertObjectType, StartInsertEvent};
use crate::ui::fuzzy_palette::{
    draw_fuzzy_palette, fuzzy_filter, PaletteConfig, PaletteItem, PaletteResult, PaletteState,
};
use crate::ui::insert_preview::{InsertPreviewKind, InsertPreviewState};
use crate::ui::theme::colors;

use super::commands::{CommandAction, CommandEvents, CommandRegistry};
use super::CommandPaletteState;

/// Item for the insert object palette
struct InsertItem {
    name: String,
    category: String,
    keywords: Vec<String>,
    action: CommandAction,
}

impl PaletteItem for InsertItem {
    fn label(&self) -> &str {
        &self.name
    }

    fn category(&self) -> Option<&str> {
        Some(&self.category)
    }

    fn keywords(&self) -> &[String] {
        &self.keywords
    }
}

/// Map a `CommandAction` to an `InsertPreviewKind` for the 3D preview.
fn action_to_preview_kind(action: &CommandAction) -> Option<InsertPreviewKind> {
    match action {
        CommandAction::SpawnPrimitive(shape) => Some(InsertPreviewKind::Primitive(*shape)),
        CommandAction::SpawnPointLight => Some(InsertPreviewKind::PointLight),
        CommandAction::SpawnDirectionalLight => Some(InsertPreviewKind::DirectionalLight),
        CommandAction::SpawnGroup => Some(InsertPreviewKind::Group),
        CommandAction::SpawnSpline(_) => Some(InsertPreviewKind::Spline),
        CommandAction::SpawnFogVolume => Some(InsertPreviewKind::FogVolume),
        CommandAction::SpawnStairs => Some(InsertPreviewKind::Stairs),
        CommandAction::SpawnRamp => Some(InsertPreviewKind::Ramp),
        CommandAction::SpawnArch => Some(InsertPreviewKind::Arch),
        CommandAction::SpawnLShape => Some(InsertPreviewKind::LShape),
        _ => None,
    }
}

/// Draw the insert object palette with a 3D preview panel.
pub(super) fn draw_insert_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    registry: &Res<CommandRegistry>,
    insert_preview_state: &mut ResMut<InsertPreviewState>,
    events: &mut CommandEvents,
    next_mode: &mut ResMut<NextState<EditorMode>>,
) -> Result {
    // Build insert item list from registry
    let items: Vec<InsertItem> = registry
        .commands
        .iter()
        .filter(|cmd| cmd.insertable)
        .map(|cmd| InsertItem {
            name: cmd.name.clone(),
            category: cmd.category.to_string(),
            keywords: cmd.keywords.clone(),
            action: cmd.action.clone(),
        })
        .collect();

    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState {
        query: std::mem::take(&mut state.query),
        selected_index: state.selected_index,
        just_opened: state.just_opened,
    };

    // Determine highlighted item for the preview
    let filtered = fuzzy_filter(&items, &palette_state.query);
    let clamped = if filtered.is_empty() {
        0
    } else {
        palette_state.selected_index.min(filtered.len() - 1)
    };

    // Update the 3D preview to match the highlighted item
    let preview_kind = filtered
        .get(clamped)
        .and_then(|fi| action_to_preview_kind(&fi.item.action));
    insert_preview_state.current_kind = preview_kind;

    // Capture preview info for the panel closure
    let preview_texture_id = insert_preview_state.texture.egui_texture_id;
    let preview_name = filtered.get(clamped).map(|fi| fi.item.name.clone());

    let has_preview = insert_preview_state.current_kind.is_some();
    let preview_panel: Option<Box<dyn FnOnce(&mut egui::Ui) + '_>> =
        Some(Box::new(move |ui: &mut egui::Ui| {
            ui.label(
                egui::RichText::new("Preview")
                    .small()
                    .strong()
                    .color(colors::TEXT_SECONDARY),
            );
            ui.add_space(4.0);
            if has_preview {
                if let Some(tex_id) = preview_texture_id {
                    let size = ui.available_width().min(220.0);
                    ui.image(egui::load::SizedTexture::new(tex_id, [size, size]));
                }
            } else {
                let size = ui.available_width().min(220.0);
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "No preview available",
                    egui::FontId::proportional(13.0),
                    colors::TEXT_MUTED,
                );
            }
            if let Some(name) = &preview_name {
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(name)
                        .color(colors::TEXT_PRIMARY)
                        .strong(),
                );
            }
        }));

    let config = PaletteConfig {
        title: "INSERT",
        title_color: colors::ACCENT_GREEN,
        subtitle: "Select object, then click to place",
        hint_text: "Type to search objects...",
        action_label: "insert",
        size: [340.0, 340.0],
        show_categories: true,
        preview_panel,
        ..Default::default()
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &items, config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            let action = &items[index].action;
            match action {
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
                    state.open_asset_browser_insert_gltf();
                    next_mode.set(EditorMode::View);
                }
                CommandAction::InsertScene => {
                    state.open_asset_browser_insert_scene();
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
                CommandAction::SpawnStairs => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Stairs,
                    });
                }
                CommandAction::SpawnRamp => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Ramp,
                    });
                }
                CommandAction::SpawnArch => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::Arch,
                    });
                }
                CommandAction::SpawnLShape => {
                    events.start_insert.write(StartInsertEvent {
                        object_type: InsertObjectType::LShape,
                    });
                }
                _ => {}
            }
            state.open = false;
            insert_preview_state.current_kind = None;
        }
        PaletteResult::Closed => {
            state.open = false;
            insert_preview_state.current_kind = None;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}
