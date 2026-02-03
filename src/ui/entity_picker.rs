use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts};

use super::fuzzy_palette::{draw_fuzzy_palette, PaletteConfig, PaletteItem, PaletteResult, PaletteState};
use super::theme::colors;
use crate::scene::SceneEntity;

/// State for the entity picker popup
#[derive(Resource, Default)]
pub struct EntityPickerState {
    /// Whether the picker is open
    pub open: bool,
    /// The palette state for fuzzy search
    pub palette_state: PaletteState,
    /// The entity being edited (that contains the Entity field)
    pub editing_entity: Option<Entity>,
    /// Field name being edited (for display/identification)
    pub field_name: String,
    /// Callback identifier to know which field to update
    pub callback_id: u64,
}

impl EntityPickerState {
    /// Open the entity picker for a specific field
    pub fn open_for_field(&mut self, editing_entity: Entity, field_name: &str, callback_id: u64) {
        self.open = true;
        self.palette_state.reset();
        self.editing_entity = Some(editing_entity);
        self.field_name = field_name.to_string();
        self.callback_id = callback_id;
    }

    /// Close the picker
    pub fn close(&mut self) {
        self.open = false;
        self.editing_entity = None;
        self.field_name.clear();
        self.callback_id = 0;
    }
}

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

pub struct EntityPickerPlugin;

impl Plugin for EntityPickerPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EntityPickerState>()
            .init_resource::<PendingEntitySelection>()
            .init_resource::<CurrentInspectedEntity>()
            .init_resource::<PendingEntityPickerRequest>()
            .add_systems(Update, draw_entity_picker);
    }
}

/// Draw the entity picker popup
fn draw_entity_picker(
    mut contexts: EguiContexts,
    mut state: ResMut<EntityPickerState>,
    mut pending_selection: ResMut<PendingEntitySelection>,
    scene_entities: Query<(Entity, &Name), With<SceneEntity>>,
) {
    if !state.open {
        return;
    }

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };

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
        state.close();
        return;
    }

    let config = PaletteConfig {
        title: "SELECT ENTITY",
        title_color: colors::ACCENT_CYAN,
        subtitle: &format!("for {}", state.field_name),
        hint_text: "Search entities...",
        action_label: "select",
        size: [350.0, 300.0],
        show_categories: false,
    };

    match draw_fuzzy_palette(ctx, &mut state.palette_state, &entities, &config) {
        PaletteResult::Selected(index) => {
            if let Some(entry) = entities.get(index) {
                // Store the selection for the inspector to consume
                pending_selection.0 = Some(EntityPickerSelection {
                    selected_entity: entry.entity,
                    callback_id: state.callback_id,
                });
            }
            state.close();
        }
        PaletteResult::Closed => {
            state.close();
        }
        PaletteResult::Open => {}
    }
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

/// Resource to track the current entity being inspected (for reflection editor context)
#[derive(Resource, Default)]
pub struct CurrentInspectedEntity(pub Option<Entity>);

/// Resource to signal that an entity picker should be opened for a reflection-based field
#[derive(Resource, Default)]
pub struct PendingEntityPickerRequest {
    pub field_path: Option<String>,
}
