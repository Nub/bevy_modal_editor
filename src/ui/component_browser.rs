use bevy::prelude::*;
use bevy::reflect::TypeRegistry;
use bevy_egui::egui;
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::any::TypeId;

use super::theme::colors;

/// Resource to track component browser state
#[derive(Resource, Default)]
pub struct ComponentBrowserState {
    /// Whether the browser is open
    pub open: bool,
    /// Search query
    pub query: String,
    /// Selected component index
    pub selected_index: usize,
    /// Whether we just opened (to focus input)
    pub just_opened: bool,
    /// Target entity to add component to
    pub target_entity: Option<Entity>,
}

impl ComponentBrowserState {
    /// Open the browser for a specific entity
    pub fn open_for_entity(&mut self, entity: Entity) {
        self.open = true;
        self.query.clear();
        self.selected_index = 0;
        self.just_opened = true;
        self.target_entity = Some(entity);
    }
}

/// Cached information about a component type
#[derive(Clone)]
pub struct ComponentInfo {
    /// Type ID
    pub type_id: TypeId,
    /// Full type name
    pub type_name: String,
    /// Short display name
    pub short_name: String,
    /// Category (derived from module path)
    pub category: String,
    /// Whether it can be instantiated with defaults
    pub can_instantiate: bool,
    /// Documentation string from Reflect
    pub docs: Option<String>,
}

/// Resource caching available components
#[derive(Resource, Default)]
pub struct ComponentRegistry {
    /// All discovered components
    pub components: Vec<ComponentInfo>,
    /// Whether the registry has been populated
    pub populated: bool,
}

impl ComponentRegistry {
    /// Populate the registry from the type registry
    pub fn populate(&mut self, type_registry: &TypeRegistry) {
        if self.populated {
            return;
        }

        self.components.clear();

        for registration in type_registry.iter() {
            // Check if this type has ReflectComponent data
            if registration.data::<ReflectComponent>().is_none() {
                continue;
            }

            let type_name = registration.type_info().type_path().to_string();
            let short_name = registration.type_info().type_path_table().short_path().to_string();

            // Extract category from module path
            let category = extract_category(&type_name);

            // Check if we can create a default instance
            let can_instantiate = registration.data::<ReflectDefault>().is_some()
                || registration.data::<ReflectFromWorld>().is_some();

            let docs = registration.type_info().docs().map(|d| d.trim().to_string());

            self.components.push(ComponentInfo {
                type_id: registration.type_id(),
                type_name,
                short_name,
                category,
                can_instantiate,
                docs,
            });
        }

        // Sort by category, then by name
        self.components.sort_by(|a, b| {
            a.category.cmp(&b.category).then(a.short_name.cmp(&b.short_name))
        });

        self.populated = true;
        info!("Component registry populated with {} components", self.components.len());
    }

    /// Get filtered components based on search query
    pub fn filter(&self, query: &str) -> Vec<(usize, &ComponentInfo, i64)> {
        let matcher = SkimMatcherV2::default();

        if query.is_empty() {
            return self
                .components
                .iter()
                .enumerate()
                .map(|(idx, info)| (idx, info, 0i64))
                .collect();
        }

        let mut results: Vec<(usize, &ComponentInfo, i64)> = self
            .components
            .iter()
            .enumerate()
            .filter_map(|(idx, info)| {
                // Match against short name
                if let Some(score) = matcher.fuzzy_match(&info.short_name, query) {
                    return Some((idx, info, score));
                }
                // Match against full type name with lower priority
                if let Some(score) = matcher.fuzzy_match(&info.type_name, query) {
                    return Some((idx, info, score / 2));
                }
                None
            })
            .collect();

        results.sort_by(|a, b| b.2.cmp(&a.2));
        results
    }
}

/// Extract a category from a type path (e.g., "bevy_transform::components::Transform" -> "Transform")
fn extract_category(type_path: &str) -> String {
    // Try to extract the crate name
    if let Some(first_segment) = type_path.split("::").next() {
        // Clean up common prefixes
        let cleaned = first_segment
            .trim_start_matches("bevy_")
            .trim_start_matches("avian3d_");

        // Capitalize first letter
        let mut chars = cleaned.chars();
        match chars.next() {
            None => "Other".to_string(),
            Some(first) => first.to_uppercase().chain(chars).collect(),
        }
    } else {
        "Other".to_string()
    }
}

pub struct ComponentBrowserPlugin;

impl Plugin for ComponentBrowserPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ComponentBrowserState>()
            .init_resource::<ComponentRegistry>();
    }
}

/// Open the component browser for a specific entity
pub fn open_component_browser(state: &mut ComponentBrowserState, entity: Entity) {
    state.open_for_entity(entity);
}

/// Draw the component browser window
pub fn draw_component_browser(
    world: &mut World,
    ctx: &egui::Context,
) -> Option<(Entity, TypeId)> {
    // First, check if browser is open and get needed data
    let (is_open, query, selected_index, just_opened, target_entity) = {
        let state = world.resource::<ComponentBrowserState>();
        (state.open, state.query.clone(), state.selected_index, state.just_opened, state.target_entity)
    };

    if !is_open {
        return None;
    }

    // Ensure registry is populated
    {
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();
        let mut registry = world.resource_mut::<ComponentRegistry>();
        registry.populate(&type_registry);
    }

    // Get filtered components
    let (filtered_data, component_count): (Vec<(usize, ComponentInfo, i64)>, usize) = {
        let registry = world.resource::<ComponentRegistry>();
        let filtered: Vec<_> = registry.filter(&query)
            .into_iter()
            .map(|(idx, info, score)| (idx, info.clone(), score))
            .collect();
        let count = filtered.len();
        (filtered, count)
    };

    // Clamp selected index
    let selected_index = if component_count > 0 {
        selected_index.min(component_count - 1)
    } else {
        0
    };

    // Update selected index in state
    {
        let mut state = world.resource_mut::<ComponentBrowserState>();
        state.selected_index = selected_index;
    }

    let mut should_close = false;
    let mut component_to_add: Option<(Entity, TypeId)> = None;

    // Handle keyboard input
    let enter_pressed = ctx.input(|i| i.key_pressed(egui::Key::Enter));
    let escape_pressed = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    let down_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowDown));
    let up_pressed = ctx.input(|i| i.key_pressed(egui::Key::ArrowUp));

    if escape_pressed {
        should_close = true;
    }

    if down_pressed && component_count > 0 {
        let mut state = world.resource_mut::<ComponentBrowserState>();
        state.selected_index = (state.selected_index + 1).min(component_count - 1);
    }
    if up_pressed {
        let mut state = world.resource_mut::<ComponentBrowserState>();
        state.selected_index = state.selected_index.saturating_sub(1);
    }

    if enter_pressed {
        if let Some((_, info, _)) = filtered_data.get(selected_index) {
            if info.can_instantiate {
                if let Some(entity) = target_entity {
                    component_to_add = Some((entity, info.type_id));
                    should_close = true;
                }
            }
        }
    }

    // Draw the window
    egui::Window::new("Add Component")
        .collapsible(false)
        .resizable(false)
        .title_bar(true)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .fixed_size([400.0, 350.0])
        .show(ctx, |ui| {
            // Search input
            let mut query_local = query.clone();
            let response = ui.add(
                egui::TextEdit::singleline(&mut query_local)
                    .hint_text("Search components...")
                    .desired_width(f32::INFINITY),
            );

            // Update query in state if changed
            if query_local != query {
                let mut state = world.resource_mut::<ComponentBrowserState>();
                state.query = query_local;
                state.selected_index = 0;
            }

            // Focus input when just opened
            if just_opened {
                response.request_focus();
                let mut state = world.resource_mut::<ComponentBrowserState>();
                state.just_opened = false;
            }

            ui.separator();

            // Get current selected index
            let current_selected = world.resource::<ComponentBrowserState>().selected_index;

            // Component list
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .show(ui, |ui| {
                    if filtered_data.is_empty() {
                        ui.label(
                            egui::RichText::new("No matching components")
                                .color(colors::TEXT_MUTED)
                                .italics(),
                        );
                    } else {
                        let mut current_category: Option<&str> = None;

                        for (display_idx, (_, info, _)) in filtered_data.iter().enumerate() {
                            // Category header
                            if current_category != Some(&info.category) {
                                current_category = Some(&info.category);
                                ui.add_space(4.0);
                                ui.label(
                                    egui::RichText::new(&info.category)
                                        .small()
                                        .color(colors::TEXT_MUTED),
                                );
                            }

                            let is_selected = display_idx == current_selected;
                            let can_add = info.can_instantiate;

                            let text_color = if !can_add {
                                colors::TEXT_MUTED
                            } else if is_selected {
                                colors::TEXT_PRIMARY
                            } else {
                                colors::TEXT_SECONDARY
                            };

                            let label_text = if can_add {
                                info.short_name.clone()
                            } else {
                                format!("{} (no default)", info.short_name)
                            };

                            let response = ui.selectable_label(
                                is_selected,
                                egui::RichText::new(&label_text).color(text_color),
                            );

                            if response.clicked() && can_add {
                                if let Some(entity) = target_entity {
                                    component_to_add = Some((entity, info.type_id));
                                    should_close = true;
                                }
                            }

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
                ui.label(egui::RichText::new("to add").small().color(colors::TEXT_MUTED));
                ui.add_space(10.0);
                ui.label(egui::RichText::new("Esc").small().strong().color(colors::ACCENT_BLUE));
                ui.label(egui::RichText::new("to close").small().color(colors::TEXT_MUTED));
            });
        });

    if should_close {
        let mut state = world.resource_mut::<ComponentBrowserState>();
        state.open = false;
        state.target_entity = None;
    }

    component_to_add
}

/// Add a component to an entity using reflection
pub fn add_component_by_type_id(world: &mut World, entity: Entity, type_id: TypeId) -> bool {
    // First, try to get component using ReflectDefault
    let component_opt: Option<Box<dyn Reflect>> = {
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();

        let Some(registration) = type_registry.get(type_id) else {
            warn!("Type not found in registry");
            return false;
        };

        if let Some(reflect_default) = registration.data::<ReflectDefault>() {
            Some(reflect_default.default())
        } else {
            None
        }
    };

    // If we got a component from ReflectDefault, insert it
    if let Some(component) = component_opt {
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();

        let Some(registration) = type_registry.get(type_id) else {
            return false;
        };

        let Some(reflect_component) = registration.data::<ReflectComponent>() else {
            warn!("Type is not a component");
            return false;
        };

        reflect_component.insert(&mut world.entity_mut(entity), component.as_ref(), &type_registry);

        let short_name = registration.type_info().type_path_table().short_path();
        info!("Added component {} to entity {:?}", short_name, entity);
        return true;
    }

    // Try ReflectFromWorld
    let has_from_world = {
        let type_registry = world.resource::<AppTypeRegistry>().clone();
        let type_registry = type_registry.read();
        type_registry
            .get(type_id)
            .and_then(|r| r.data::<ReflectFromWorld>())
            .is_some()
    };

    if has_from_world {
        let type_registry = world.resource::<AppTypeRegistry>().clone();

        let component = {
            let type_registry = type_registry.read();
            let registration = type_registry.get(type_id).unwrap();
            let reflect_from_world = registration.data::<ReflectFromWorld>().unwrap();
            reflect_from_world.from_world(world)
        };

        let type_registry = type_registry.read();
        let registration = type_registry.get(type_id).unwrap();
        let reflect_component = registration.data::<ReflectComponent>().unwrap();

        reflect_component.insert(&mut world.entity_mut(entity), component.as_ref(), &type_registry);

        let short_name = registration.type_info().type_path_table().short_path();
        info!("Added component {} to entity {:?}", short_name, entity);
        return true;
    }

    warn!("Cannot create default instance of component");
    false
}
