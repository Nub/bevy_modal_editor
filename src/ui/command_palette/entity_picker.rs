//! Entity picker palette — select an entity for a field reference.

use bevy::prelude::*;
use bevy_egui::egui;

use bevy_editor_game::GameEntity;

use crate::scene::SceneEntity;
use crate::ui::fuzzy_palette::{draw_fuzzy_palette, PaletteConfig, PaletteItem, PaletteResult, PaletteState};
use crate::ui::theme::colors;

use super::CommandPaletteState;

/// Result of entity picker selection
#[derive(Clone, Copy, Debug)]
pub struct EntityPickerSelection {
    /// The entity that was selected
    pub selected_entity: Entity,
    /// The callback ID to identify which field to update
    pub callback_id: u64,
}

/// Resource to store the pending entity selection (set when user picks an entity)
#[derive(Resource, Default)]
pub struct PendingEntitySelection(pub Option<EntityPickerSelection>);

/// Resource to track the current entity being inspected (for reflection editor context)
#[derive(Resource, Default)]
pub struct CurrentInspectedEntity(pub Option<Entity>);

/// Resource to signal that an entity picker should be opened for a reflection-based field
#[derive(Resource, Default)]
pub struct PendingEntityPickerRequest {
    pub field_path: Option<String>,
}

/// Entry for an entity in the picker
struct EntityEntry {
    entity: Entity,
    name: String,
}

impl PaletteItem for EntityEntry {
    fn label(&self) -> &str {
        &self.name
    }

    fn category(&self) -> Option<&str> {
        None
    }

    fn is_enabled(&self) -> bool {
        true
    }

    fn suffix(&self) -> Option<&str> {
        None
    }

    fn keywords(&self) -> &[String] {
        &[]
    }
}

/// Draw the entity picker popup
pub(super) fn draw_entity_picker(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    pending_selection: &mut ResMut<PendingEntitySelection>,
    scene_entities: &Query<(Entity, &Name), Or<(With<SceneEntity>, With<GameEntity>)>>,
) -> Result {
    // Build list of entities
    let entities: Vec<EntityEntry> = scene_entities
        .iter()
        .map(|(entity, name)| EntityEntry {
            entity,
            name: name.as_str().to_string(),
        })
        .collect();

    // Check for escape to close
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        state.open = false;
        return Ok(());
    }

    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState {
        query: std::mem::take(&mut state.query),
        selected_index: state.selected_index,
        just_opened: state.just_opened,
    };

    let field_name = state.picker_field_name.clone();
    let config = PaletteConfig {
        title: "SELECT ENTITY",
        title_color: colors::ACCENT_CYAN,
        subtitle: &format!("for {}", field_name),
        hint_text: "Search entities...",
        action_label: "select",
        size: [350.0, 300.0],
        show_categories: false,
        preview_panel: None,
        ..Default::default()
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &entities, config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            if let Some(entry) = entities.get(index) {
                // Store the selection for the inspector to consume
                pending_selection.0 = Some(EntityPickerSelection {
                    selected_entity: entry.entity,
                    callback_id: state.picker_callback_id,
                });
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

/// Helper function to draw an entity field with a picker button
/// Returns true if the field was clicked and the picker should open
pub fn draw_entity_field(
    ui: &mut egui::Ui,
    label: &str,
    current_entity: Entity,
    entity_name: Option<&str>,
) -> bool {
    let mut clicked = false;

    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).color(colors::TEXT_SECONDARY));

        let display_text = entity_name
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("{:?}", current_entity));

        let button = ui.add(
            egui::Button::new(
                egui::RichText::new(&display_text)
                    .color(colors::ACCENT_CYAN)
                    .small(),
            )
            .frame(true),
        );

        if button.clicked() {
            clicked = true;
        }

        button.on_hover_text("Click to select entity");
    });

    clicked
}

/// Helper to generate a unique callback ID from entity and field name
pub fn make_callback_id(entity: Entity, field_name: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    entity.hash(&mut hasher);
    field_name.hash(&mut hasher);
    hasher.finish()
}
