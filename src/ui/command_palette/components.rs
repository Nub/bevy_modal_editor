//! Component search, add, and remove palettes.

use bevy::prelude::*;
use bevy_egui::egui;
use std::any::TypeId;

use crate::commands::TakeSnapshotCommand;
use crate::selection::Selected;
use crate::ui::component_browser::add_component_by_type_id;
use crate::ui::fuzzy_palette::{
    draw_fuzzy_palette, fuzzy_filter, PaletteConfig, PaletteItem, PaletteResult, PaletteState,
};
use crate::ui::theme::colors;

use super::{CommandPaletteState, PaletteMode, RemovableComponentsCache};

/// Resource containing available components for adding
#[derive(Resource, Default)]
pub struct ComponentRegistry {
    pub components: Vec<ComponentInfo>,
    populated: bool,
}

/// Information about a registerable component
pub struct ComponentInfo {
    pub type_id: TypeId,
    pub short_name: String,
    pub type_name: String,
    pub category: String,
    pub can_instantiate: bool,
    pub docs: Option<String>,
}

impl ComponentRegistry {
    /// Populate the registry from the type registry (only once)
    pub fn populate(&mut self, type_registry: &bevy::reflect::TypeRegistry) {
        if self.populated {
            return;
        }
        self.populated = true;

        self.components = type_registry
            .iter()
            .filter_map(|registration| {
                // Only include types with ReflectComponent
                registration.data::<ReflectComponent>()?;
                let type_info = registration.type_info();
                let short_name = type_info.type_path_table().short_path().to_string();
                let type_name = type_info.type_path().to_string();
                let docs = type_info.docs().map(|d| d.trim().to_string());

                // Determine category from module path
                let category = type_name
                    .rsplit("::")
                    .nth(1)
                    .unwrap_or("Other")
                    .to_string();

                // Check if it has FromReflect (can be instantiated with defaults)
                let can_instantiate = registration.data::<ReflectFromReflect>().is_some()
                    || registration.data::<ReflectDefault>().is_some();

                Some(ComponentInfo {
                    type_id: registration.type_id(),
                    short_name,
                    type_name,
                    category,
                    can_instantiate,
                    docs,
                })
            })
            .collect();

        self.components.sort_by(|a, b| a.short_name.cmp(&b.short_name));
    }
}

/// Wrapper for component info to implement PaletteItem
struct ComponentSearchItem {
    type_id: TypeId,
    name: String,
    type_path: String,
    docs: Option<String>,
}

impl PaletteItem for ComponentSearchItem {
    fn label(&self) -> &str {
        &self.name
    }
}

/// Draw a component documentation preview panel.
fn draw_component_doc_preview(ui: &mut egui::Ui, name: &str, type_path: &str, docs: Option<&str>) {
    ui.label(
        egui::RichText::new(name)
            .strong()
            .color(colors::TEXT_PRIMARY),
    );
    ui.add_space(2.0);
    ui.label(
        egui::RichText::new(type_path)
            .small()
            .color(colors::TEXT_MUTED),
    );
    ui.add_space(8.0);

    if let Some(doc_text) = docs {
        let mut cache = egui_commonmark::CommonMarkCache::default();
        egui_commonmark::CommonMarkViewer::new()
            .max_image_width(Some(0))
            .show_scrollable("component_doc_preview", ui, &mut cache, doc_text);
    } else {
        ui.label(
            egui::RichText::new("No documentation available")
                .small()
                .italics()
                .color(colors::TEXT_MUTED),
        );
    }
}

/// Draw the component search palette for ObjectInspector mode
pub(super) fn draw_component_search_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    component_editor_state: &mut ResMut<super::super::inspector::ComponentEditorState>,
    type_registry: &Res<AppTypeRegistry>,
    selected: &Query<Entity, With<Selected>>,
) -> Result {
    // Get selected entity
    let Some(_entity) = selected.iter().next() else {
        state.open = false;
        return Ok(());
    };

    // Build list of components from the type registry
    let type_registry_guard = type_registry.read();
    let mut components: Vec<ComponentSearchItem> = type_registry_guard
        .iter()
        .filter_map(|registration| {
            registration.data::<ReflectComponent>()?;
            let type_id = registration.type_id();
            let type_info = registration.type_info();
            let short_name = type_info.type_path_table().short_path().to_string();
            let type_path = type_info.type_path().to_string();
            let docs = type_info.docs().map(|d| d.trim().to_string());
            Some(ComponentSearchItem {
                type_id,
                name: short_name,
                type_path,
                docs,
            })
        })
        .collect();
    components.sort_by(|a, b| a.name.cmp(&b.name));
    drop(type_registry_guard);

    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState {
        query: std::mem::take(&mut state.query),
        selected_index: state.selected_index,
        just_opened: state.just_opened,
    };

    // Determine highlighted item for the preview
    let filtered = fuzzy_filter(&components, &palette_state.query);
    let clamped = if filtered.is_empty() {
        0
    } else {
        palette_state.selected_index.min(filtered.len() - 1)
    };
    let preview_info: Option<(String, String, Option<String>)> =
        filtered.get(clamped).map(|fi| {
            (
                fi.item.name.clone(),
                fi.item.type_path.clone(),
                fi.item.docs.clone(),
            )
        });

    let preview_panel: Option<Box<dyn FnOnce(&mut egui::Ui) + '_>> =
        Some(Box::new(move |ui: &mut egui::Ui| {
            if let Some((name, type_path, docs)) = &preview_info {
                draw_component_doc_preview(ui, name, type_path, docs.as_deref());
            }
        }));

    let config = PaletteConfig {
        title: "INSPECT MODE",
        title_color: colors::ACCENT_PURPLE,
        subtitle: "Search for component to edit",
        hint_text: "Type to search components...",
        action_label: "edit",
        size: [280.0, 350.0],
        show_categories: false,
        preview_panel,
        preview_width: 320.0,
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &components, config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            let item = &components[index];
            component_editor_state.editing_component = Some((item.type_id, item.name.clone()));
            component_editor_state.just_opened = true;
            state.open = false;
        }
        PaletteResult::Closed => {
            state.open = false;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}

/// Wrapper for adding components that implements PaletteItem
struct AddComponentItem {
    type_id: TypeId,
    short_name: String,
    category: String,
    can_instantiate: bool,
    type_path: String,
    docs: Option<String>,
}

impl PaletteItem for AddComponentItem {
    fn label(&self) -> &str {
        &self.short_name
    }

    fn category(&self) -> Option<&str> {
        Some(&self.category)
    }

    fn is_enabled(&self) -> bool {
        self.can_instantiate
    }

    fn suffix(&self) -> Option<&str> {
        if self.can_instantiate {
            None
        } else {
            Some("(no default)")
        }
    }
}

/// Draw the add component palette for adding new components to an entity
pub(super) fn draw_add_component_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    component_registry: &mut ResMut<ComponentRegistry>,
    type_registry: &Res<AppTypeRegistry>,
    commands: &mut Commands,
) -> Result {
    // Get target entity
    let Some(target_entity) = state.target_entity else {
        state.open = false;
        return Ok(());
    };

    // Ensure registry is populated
    {
        let type_registry_guard = type_registry.read();
        component_registry.populate(&type_registry_guard);
    }

    // Convert to PaletteItem wrappers
    let items: Vec<AddComponentItem> = component_registry
        .components
        .iter()
        .map(|c| AddComponentItem {
            type_id: c.type_id,
            short_name: c.short_name.clone(),
            category: c.category.clone(),
            can_instantiate: c.can_instantiate,
            type_path: c.type_name.clone(),
            docs: c.docs.clone(),
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
    let preview_info: Option<(String, String, Option<String>)> =
        filtered.get(clamped).map(|fi| {
            (
                fi.item.short_name.clone(),
                fi.item.type_path.clone(),
                fi.item.docs.clone(),
            )
        });

    let preview_panel: Option<Box<dyn FnOnce(&mut egui::Ui) + '_>> =
        Some(Box::new(move |ui: &mut egui::Ui| {
            if let Some((name, type_path, docs)) = &preview_info {
                draw_component_doc_preview(ui, name, type_path, docs.as_deref());
            }
        }));

    let config = PaletteConfig {
        title: "ADD COMPONENT",
        title_color: colors::ACCENT_GREEN,
        subtitle: "Select component to add",
        hint_text: "Type to search components...",
        action_label: "add",
        size: [280.0, 350.0],
        show_categories: false,
        preview_panel,
        preview_width: 320.0,
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &items, config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            let item = &items[index];
            // Queue a command to add the component
            commands.queue(AddComponentCommand {
                entity: target_entity,
                type_id: item.type_id,
                component_name: item.short_name.clone(),
            });
            state.open = false;
            state.target_entity = None;
        }
        PaletteResult::Closed => {
            state.open = false;
            state.target_entity = None;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}

/// Command to add a component via reflection (deferred execution)
struct AddComponentCommand {
    entity: Entity,
    type_id: TypeId,
    component_name: String,
}

impl bevy::prelude::Command for AddComponentCommand {
    fn apply(self, world: &mut World) {
        if add_component_by_type_id(world, self.entity, self.type_id) {
            // Open the component editor for the newly added component
            let mut editor_state = world.resource_mut::<super::super::inspector::ComponentEditorState>();
            editor_state.editing_component = Some((self.type_id, self.component_name));
            editor_state.just_opened = true;
        }
    }
}

/// Item for the remove component palette
struct RemoveComponentItem {
    type_id: TypeId,
    short_name: String,
}

impl PaletteItem for RemoveComponentItem {
    fn label(&self) -> &str {
        &self.short_name
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

/// Draw the remove component palette
pub(super) fn draw_remove_component_palette(
    ctx: &egui::Context,
    state: &mut ResMut<CommandPaletteState>,
    removable_cache: &Res<RemovableComponentsCache>,
    selected: &Query<Entity, With<Selected>>,
    commands: &mut Commands,
) -> Result {
    // Get target entity (either from state or first selected)
    let target_entity = state.target_entity.or_else(|| selected.iter().next());
    let Some(target_entity) = target_entity else {
        state.open = false;
        return Ok(());
    };

    // Store in state so the populate_removable_components system can fill the cache
    state.target_entity = Some(target_entity);

    // Bridge CommandPaletteState to PaletteState
    let mut palette_state = PaletteState {
        query: std::mem::take(&mut state.query),
        selected_index: state.selected_index,
        just_opened: state.just_opened,
    };

    // Use the cached component list (populated by populate_removable_components system)
    let items: Vec<RemoveComponentItem> = removable_cache
        .components
        .iter()
        .map(|(type_id, name)| RemoveComponentItem {
            type_id: *type_id,
            short_name: name.clone(),
        })
        .collect();

    let config = PaletteConfig {
        title: "REMOVE COMPONENT",
        title_color: colors::STATUS_ERROR,
        subtitle: "Select component to remove",
        hint_text: "Type to search components...",
        action_label: "remove",
        size: [400.0, 350.0],
        show_categories: false,
        preview_panel: None,
        ..Default::default()
    };

    let result = draw_fuzzy_palette(ctx, &mut palette_state, &items, config);

    // Sync state back
    state.query = palette_state.query;
    state.selected_index = palette_state.selected_index;
    state.just_opened = palette_state.just_opened;

    match result {
        PaletteResult::Selected(index) => {
            if let Some(item) = items.get(index) {
                // Queue snapshot and remove commands
                commands.queue(TakeSnapshotCommand {
                    description: format!("Remove {} component", item.short_name),
                });
                commands.queue(RemoveComponentCommand {
                    entity: target_entity,
                    type_id: item.type_id,
                    component_name: item.short_name.clone(),
                });
            }
            state.open = false;
            state.target_entity = None;
        }
        PaletteResult::Closed => {
            state.open = false;
            state.target_entity = None;
        }
        PaletteResult::Open => {}
    }

    Ok(())
}

/// Command to remove a component via reflection (deferred execution)
struct RemoveComponentCommand {
    entity: Entity,
    type_id: TypeId,
    component_name: String,
}

impl bevy::prelude::Command for RemoveComponentCommand {
    fn apply(self, world: &mut World) {
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry_guard = type_registry.read();

        // Find the component registration
        let Some(registration) = type_registry_guard.get(self.type_id) else {
            warn!("Cannot find type registration for component: {}", self.component_name);
            return;
        };

        // Get ReflectComponent to remove
        let Some(reflect_component) = registration.data::<ReflectComponent>() else {
            warn!("Component {} does not have ReflectComponent", self.component_name);
            return;
        };

        // Remove the component (entity may have been despawned)
        let Ok(mut entity_mut) = world.get_entity_mut(self.entity) else {
            warn!("Entity {:?} no longer exists, cannot remove component", self.entity);
            return;
        };
        reflect_component.remove(&mut entity_mut);
        info!("Removed component {} from entity {:?}", self.component_name, self.entity);
    }
}

/// Populate the removable components cache when RemoveComponent mode is active
pub(super) fn populate_removable_components(world: &mut World) {
    // Check if we're in RemoveComponent mode
    let state = world.resource::<CommandPaletteState>();
    if state.mode != PaletteMode::RemoveComponent || !state.open {
        return;
    }

    let target_entity = state.target_entity;
    let Some(entity) = target_entity else {
        return;
    };

    // Check if cache is already populated for this entity
    let cache = world.resource::<RemovableComponentsCache>();
    if cache.entity == Some(entity) && !cache.components.is_empty() {
        return;
    }

    // Get the type registry
    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry_guard = type_registry.read();

    // Get components on this entity
    let entity_ref = world.entity(entity);
    let archetype = entity_ref.archetype();

    let mut components: Vec<(TypeId, String)> = archetype
        .components()
        .iter()
        .filter_map(|component_id| {
            let component_info = world.components().get_info(*component_id)?;
            let type_id = component_info.type_id()?;

            // Check if this type is registered for reflection
            let registration = type_registry_guard.get(type_id)?;

            // Check if it has ReflectComponent (can be removed)
            registration.data::<ReflectComponent>()?;

            let short_name = registration
                .type_info()
                .type_path_table()
                .short_path()
                .to_string();

            // Skip core components that shouldn't be removed
            if short_name == "Transform"
                || short_name == "GlobalTransform"
                || short_name == "Visibility"
                || short_name == "InheritedVisibility"
                || short_name == "ViewVisibility"
                || short_name == "SceneEntity"
            {
                return None;
            }

            Some((type_id, short_name))
        })
        .collect();

    components.sort_by(|a, b| a.1.cmp(&b.1));

    drop(type_registry_guard);

    // Update the cache
    let mut cache = world.resource_mut::<RemovableComponentsCache>();
    cache.entity = Some(entity);
    cache.components = components;
}
