use avian3d::prelude::RigidBody;
use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};
use std::any::TypeId;

use super::component_browser::{
    add_component_by_type_id, draw_component_browser, open_component_browser, ComponentBrowserState,
};
use super::reflect_editor::{component_editor, ReflectEditorConfig};
use super::InspectorPanelState;
use crate::scene::{DirectionalLightMarker, Locked, SceneLightMarker};
use crate::selection::Selected;
use crate::ui::theme::colors;

/// Represents the RigidBody type for UI selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RigidBodyType {
    Static,
    Dynamic,
    Kinematic,
}

impl RigidBodyType {
    fn from_rigid_body(rb: &RigidBody) -> Self {
        match rb {
            RigidBody::Static => RigidBodyType::Static,
            RigidBody::Dynamic => RigidBodyType::Dynamic,
            RigidBody::Kinematic => RigidBodyType::Kinematic,
        }
    }

    fn to_rigid_body(self) -> RigidBody {
        match self {
            RigidBodyType::Static => RigidBody::Static,
            RigidBodyType::Dynamic => RigidBody::Dynamic,
            RigidBodyType::Kinematic => RigidBody::Kinematic,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            RigidBodyType::Static => "Static",
            RigidBodyType::Dynamic => "Dynamic",
            RigidBodyType::Kinematic => "Kinematic",
        }
    }

    const ALL: [RigidBodyType; 3] = [
        RigidBodyType::Static,
        RigidBodyType::Dynamic,
        RigidBodyType::Kinematic,
    ];
}

pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_inspector_panel);
    }
}

/// Result of drawing a removable component section
enum ComponentAction<T> {
    None,
    Update(T),
    Remove,
}

/// Data for point light editing
#[derive(Clone)]
struct PointLightData {
    color: [f32; 3],
    intensity: f32,
    range: f32,
    shadows_enabled: bool,
}

impl From<&SceneLightMarker> for PointLightData {
    fn from(marker: &SceneLightMarker) -> Self {
        let color = marker.color.to_srgba();
        Self {
            color: [color.red, color.green, color.blue],
            intensity: marker.intensity,
            range: marker.range,
            shadows_enabled: marker.shadows_enabled,
        }
    }
}

/// Data for directional light editing
#[derive(Clone)]
struct DirectionalLightData {
    color: [f32; 3],
    illuminance: f32,
    shadows_enabled: bool,
}

impl From<&DirectionalLightMarker> for DirectionalLightData {
    fn from(marker: &DirectionalLightMarker) -> Self {
        let color = marker.color.to_srgba();
        Self {
            color: [color.red, color.green, color.blue],
            illuminance: marker.illuminance,
            shadows_enabled: marker.shadows_enabled,
        }
    }
}

/// Draw a transform section with colored X/Y/Z labels
fn draw_transform_section(ui: &mut egui::Ui, transform: &mut Transform) -> bool {
    let mut changed = false;

    egui::CollapsingHeader::new(
        egui::RichText::new("Transform").strong().color(colors::TEXT_PRIMARY),
    )
    .default_open(true)
    .show(ui, |ui| {
        ui.add_space(4.0);

        // Translation
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Translation").color(colors::TEXT_SECONDARY));
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("X").color(colors::AXIS_X).strong());
            changed |= ui
                .add(egui::DragValue::new(&mut transform.translation.x).speed(0.1))
                .changed();
            ui.label(egui::RichText::new("Y").color(colors::AXIS_Y).strong());
            changed |= ui
                .add(egui::DragValue::new(&mut transform.translation.y).speed(0.1))
                .changed();
            ui.label(egui::RichText::new("Z").color(colors::AXIS_Z).strong());
            changed |= ui
                .add(egui::DragValue::new(&mut transform.translation.z).speed(0.1))
                .changed();
        });

        ui.add_space(4.0);

        // Rotation (as euler angles in degrees)
        let (mut yaw, mut pitch, mut roll) = transform.rotation.to_euler(EulerRot::YXZ);
        yaw = yaw.to_degrees();
        pitch = pitch.to_degrees();
        roll = roll.to_degrees();

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Rotation").color(colors::TEXT_SECONDARY));
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("X").color(colors::AXIS_X).strong());
            let x_changed = ui
                .add(egui::DragValue::new(&mut pitch).speed(1.0).suffix("°"))
                .changed();
            ui.label(egui::RichText::new("Y").color(colors::AXIS_Y).strong());
            let y_changed = ui
                .add(egui::DragValue::new(&mut yaw).speed(1.0).suffix("°"))
                .changed();
            ui.label(egui::RichText::new("Z").color(colors::AXIS_Z).strong());
            let z_changed = ui
                .add(egui::DragValue::new(&mut roll).speed(1.0).suffix("°"))
                .changed();

            if x_changed || y_changed || z_changed {
                transform.rotation = Quat::from_euler(
                    EulerRot::YXZ,
                    yaw.to_radians(),
                    pitch.to_radians(),
                    roll.to_radians(),
                );
                changed = true;
            }
        });

        ui.add_space(4.0);

        // Scale
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Scale").color(colors::TEXT_SECONDARY));
        });
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("X").color(colors::AXIS_X).strong());
            changed |= ui
                .add(egui::DragValue::new(&mut transform.scale.x).speed(0.01))
                .changed();
            ui.label(egui::RichText::new("Y").color(colors::AXIS_Y).strong());
            changed |= ui
                .add(egui::DragValue::new(&mut transform.scale.y).speed(0.01))
                .changed();
            ui.label(egui::RichText::new("Z").color(colors::AXIS_Z).strong());
            changed |= ui
                .add(egui::DragValue::new(&mut transform.scale.z).speed(0.01))
                .changed();
        });

        ui.add_space(4.0);
    });

    changed
}

/// Draw point light properties section
fn draw_point_light_section(ui: &mut egui::Ui, data: &mut PointLightData) -> bool {
    let mut changed = false;

    egui::CollapsingHeader::new(
        egui::RichText::new("Point Light").strong().color(colors::TEXT_PRIMARY),
    )
    .default_open(true)
    .show(ui, |ui| {
        ui.add_space(4.0);

        // Color picker
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Color").color(colors::TEXT_SECONDARY));
            changed |= ui.color_edit_button_rgb(&mut data.color).changed();
        });

        ui.add_space(4.0);

        // Intensity
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Intensity").color(colors::TEXT_SECONDARY));
            changed |= ui
                .add(egui::DragValue::new(&mut data.intensity).speed(100.0).range(0.0..=1000000.0))
                .changed();
        });

        ui.add_space(4.0);

        // Range
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Range").color(colors::TEXT_SECONDARY));
            changed |= ui
                .add(egui::DragValue::new(&mut data.range).speed(0.1).range(0.0..=1000.0))
                .changed();
        });

        ui.add_space(4.0);

        // Shadows
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Shadows").color(colors::TEXT_SECONDARY));
            changed |= ui.checkbox(&mut data.shadows_enabled, "").changed();
        });

        ui.add_space(4.0);
    });

    changed
}

/// Draw directional light properties section
fn draw_directional_light_section(ui: &mut egui::Ui, data: &mut DirectionalLightData) -> bool {
    let mut changed = false;

    egui::CollapsingHeader::new(
        egui::RichText::new("Directional Light").strong().color(colors::TEXT_PRIMARY),
    )
    .default_open(true)
    .show(ui, |ui| {
        ui.add_space(4.0);

        // Color picker
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Color").color(colors::TEXT_SECONDARY));
            changed |= ui.color_edit_button_rgb(&mut data.color).changed();
        });

        ui.add_space(4.0);

        // Illuminance
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Illuminance").color(colors::TEXT_SECONDARY));
            changed |= ui
                .add(egui::DragValue::new(&mut data.illuminance).speed(100.0).range(0.0..=200000.0))
                .changed();
        });

        ui.add_space(4.0);

        // Shadows
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Shadows").color(colors::TEXT_SECONDARY));
            changed |= ui.checkbox(&mut data.shadows_enabled, "").changed();
        });

        ui.add_space(4.0);
    });

    changed
}

/// Draw a RigidBody type selector with remove button
/// current_type is None if entities have mixed types
fn draw_rigidbody_section(ui: &mut egui::Ui, current_type: Option<RigidBodyType>) -> ComponentAction<RigidBodyType> {
    let mut action = ComponentAction::None;

    ui.horizontal(|ui| {
        ui.collapsing(
            egui::RichText::new("Physics").strong().color(colors::TEXT_PRIMARY),
            |ui| {
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Body Type").color(colors::TEXT_SECONDARY));
                });

                let display_text = current_type.map(|t| t.label()).unwrap_or("Mixed");

                let mut new_type = None;
                egui::ComboBox::from_id_salt("rigidbody_type")
                    .selected_text(display_text)
                    .show_ui(ui, |ui| {
                        for rb_type in RigidBodyType::ALL {
                            if ui.selectable_value(&mut new_type, Some(rb_type), rb_type.label()).clicked() {
                                // Only set if different from current
                                if current_type != Some(rb_type) {
                                    new_type = Some(rb_type);
                                } else {
                                    new_type = None;
                                }
                            }
                        }
                    });

                if let Some(t) = new_type {
                    action = ComponentAction::Update(t);
                }

                ui.add_space(4.0);
            },
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("X")
                .on_hover_text("Remove Physics component")
                .clicked()
            {
                action = ComponentAction::Remove;
            }
        });
    });

    action
}

/// Draw the component inspector panel
fn draw_inspector_panel(world: &mut World) {
    // Query for all selected entities
    let selected_entities: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<Selected>>();
        query.iter(world).collect()
    };

    let selection_count = selected_entities.len();
    let single_entity = if selection_count == 1 { Some(selected_entities[0]) } else { None };

    // Get entity name and transform for single selection
    let mut entity_name = single_entity.and_then(|e| {
        world.get::<Name>(e).map(|n| n.as_str().to_string())
    });
    let mut transform_copy = single_entity.and_then(|e| world.get::<Transform>(e).copied());
    let original_name = entity_name.clone();

    // Get locked state for single selection
    let mut is_locked = single_entity.map(|e| world.get::<Locked>(e).is_some()).unwrap_or(false);
    let original_locked = is_locked;

    // Get RigidBody types for all selected entities that have one
    let rigidbody_types: Vec<(Entity, RigidBodyType)> = selected_entities
        .iter()
        .filter_map(|&e| {
            world.get::<RigidBody>(e).map(|rb| (e, RigidBodyType::from_rigid_body(rb)))
        })
        .collect();

    // Determine if all have same type or mixed
    let common_rigidbody_type: Option<RigidBodyType> = if rigidbody_types.is_empty() {
        None
    } else {
        let first_type = rigidbody_types[0].1;
        if rigidbody_types.iter().all(|(_, t)| *t == first_type) {
            Some(first_type)
        } else {
            None // Mixed types
        }
    };
    let has_rigidbodies = !rigidbody_types.is_empty();

    // Get point light data for single selection
    let mut point_light_data = single_entity.and_then(|e| {
        world.get::<SceneLightMarker>(e).map(|m| PointLightData::from(m))
    });

    // Get directional light data for single selection
    let mut directional_light_data = single_entity.and_then(|e| {
        world.get::<DirectionalLightMarker>(e).map(|m| DirectionalLightData::from(m))
    });

    // Get egui context
    let ctx = {
        let Some(mut egui_ctx) = world
            .query::<&mut bevy_egui::EguiContext>()
            .iter_mut(world)
            .next()
        else {
            return;
        };
        egui_ctx.get_mut().clone()
    };

    let mut transform_changed = false;
    let mut rigidbody_action: ComponentAction<RigidBodyType> = ComponentAction::None;
    let mut point_light_changed = false;
    let mut directional_light_changed = false;

    // Check for "N" key to focus name field (only for single selection)
    let focus_name_field = selection_count == 1
        && !ctx.wants_keyboard_input()
        && ctx.input(|i| i.key_pressed(egui::Key::N));
    let name_field_id = egui::Id::new("inspector_name_field");

    // Floating window padding from edges
    let window_padding = 8.0;
    let status_bar_height = 24.0;
    let available_height = ctx.content_rect().height() - status_bar_height - window_padding * 2.0;

    let panel_response = egui::Window::new("Inspector")
        .default_size([300.0, available_height])
        .min_height(100.0)
        .max_height(available_height)
        .anchor(egui::Align2::RIGHT_TOP, [-window_padding, window_padding])
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(colors::PANEL_BG)
                .shadow(egui::Shadow {
                    offset: [0, 2],
                    blur: 4,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(40),
                }),
        )
        .show(&ctx, |ui| {
            // Force the window content to fill available height
            let title_bar_height = 28.0;
            let bottom_padding = 30.0;
            ui.set_min_height(available_height - title_bar_height - bottom_padding);

            match selection_count {
                0 => {
                    // No selection
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("No entity selected")
                                .color(colors::TEXT_MUTED)
                                .italics(),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            egui::RichText::new("Click an entity in the viewport\nor hierarchy to select it.")
                                .small()
                                .color(colors::TEXT_MUTED),
                        );
                    });
                }
                1 => {
                    // Single selection - full inspector
                    let entity = single_entity.unwrap();
                    ui.add_space(4.0);

                    // Editable entity name
                    if let Some(ref mut name) = entity_name {
                        let response = ui.add(
                            egui::TextEdit::singleline(name)
                                .id(name_field_id)
                                .font(egui::FontId::proportional(16.0))
                                .text_color(colors::TEXT_PRIMARY)
                                .margin(egui::vec2(8.0, 6.0)),
                        );
                        if focus_name_field {
                            response.request_focus();
                        }
                    } else {
                        ui.label(
                            egui::RichText::new(format!("Entity {:?}", entity))
                                .strong()
                                .size(14.0)
                                .color(colors::TEXT_PRIMARY),
                        );
                    }
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(format!("ID: {:?}", entity))
                                .small()
                                .color(colors::TEXT_MUTED),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.checkbox(&mut is_locked, "Locked");
                        });
                    });

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        // Show locked message if entity is locked
                        if is_locked {
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("Entity is locked")
                                        .color(colors::TEXT_MUTED)
                                        .italics(),
                                );
                            });
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("Unlock to edit properties")
                                    .small()
                                    .color(colors::TEXT_MUTED),
                            );
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(4.0);
                        }

                        // Custom Transform section with colored labels (disabled if locked)
                        if !is_locked {
                            if let Some(ref mut transform) = transform_copy {
                                transform_changed = draw_transform_section(ui, transform);
                            }

                            ui.add_space(4.0);

                            // RigidBody type selector (only if entity has RigidBody)
                            if has_rigidbodies {
                                rigidbody_action = draw_rigidbody_section(ui, common_rigidbody_type);
                                ui.add_space(4.0);
                            }

                            // Point light properties
                            if let Some(ref mut data) = point_light_data {
                                point_light_changed = draw_point_light_section(ui, data);
                                ui.add_space(4.0);
                            }

                            // Directional light properties
                            if let Some(ref mut data) = directional_light_data {
                                directional_light_changed = draw_directional_light_section(ui, data);
                                ui.add_space(4.0);
                            }

                            ui.separator();
                            ui.add_space(4.0);
                        }

                        // Add Component button
                        ui.add_space(4.0);
                        if ui
                            .button(egui::RichText::new("+ Add Component").color(colors::ACCENT_GREEN))
                            .clicked()
                        {
                            let mut browser_state = world.resource_mut::<ComponentBrowserState>();
                            open_component_browser(&mut browser_state, entity);
                        }
                        ui.add_space(4.0);

                        ui.separator();
                        ui.add_space(4.0);

                        // Show all components via reflection
                        egui::CollapsingHeader::new(
                            egui::RichText::new("All Components").color(colors::TEXT_SECONDARY),
                        )
                        .default_open(false)
                        .show(ui, |ui| {
                            draw_all_components(world, entity, ui);
                        });
                    });
                }
                _ => {
                    // Multiple selection
                    ui.add_space(4.0);

                    ui.label(
                        egui::RichText::new(format!("{} entities selected", selection_count))
                            .strong()
                            .size(14.0)
                            .color(colors::TEXT_PRIMARY),
                    );

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        // RigidBody type selector for multi-selection
                        if has_rigidbodies {
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} of {} have physics",
                                    rigidbody_types.len(),
                                    selection_count
                                ))
                                .small()
                                .color(colors::TEXT_MUTED),
                            );
                            ui.add_space(4.0);
                            rigidbody_action = draw_rigidbody_section(ui, common_rigidbody_type);
                        } else {
                            ui.label(
                                egui::RichText::new("No shared properties to edit")
                                    .color(colors::TEXT_MUTED)
                                    .italics(),
                            );
                        }
                    });
                }
            }
        });

    // Apply transform changes back to the entity (single selection only)
    if transform_changed {
        if let (Some(entity), Some(new_transform)) = (single_entity, transform_copy) {
            if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                *transform = new_transform;
            }
        }
    }

    // Apply name changes back to the entity (single selection only)
    if entity_name != original_name {
        if let (Some(entity), Some(new_name)) = (single_entity, entity_name) {
            if let Some(mut name) = world.get_mut::<Name>(entity) {
                name.set(new_name);
            }
        }
    }

    // Apply locked state changes
    if is_locked != original_locked {
        if let Some(entity) = single_entity {
            if is_locked {
                world.entity_mut(entity).insert(Locked);
            } else {
                world.entity_mut(entity).remove::<Locked>();
            }
        }
    }

    // Apply RigidBody changes to all selected entities with RigidBody
    match rigidbody_action {
        ComponentAction::Update(new_type) => {
            for (entity, _) in &rigidbody_types {
                world.entity_mut(*entity).remove::<RigidBody>();
                world.entity_mut(*entity).insert(new_type.to_rigid_body());
            }
        }
        ComponentAction::Remove => {
            for (entity, _) in &rigidbody_types {
                world.entity_mut(*entity).remove::<RigidBody>();
            }
        }
        ComponentAction::None => {}
    }

    // Apply point light changes
    if point_light_changed {
        if let (Some(entity), Some(data)) = (single_entity, point_light_data) {
            let new_color = Color::srgb(data.color[0], data.color[1], data.color[2]);

            // Update the marker component
            if let Some(mut marker) = world.get_mut::<SceneLightMarker>(entity) {
                marker.color = new_color;
                marker.intensity = data.intensity;
                marker.range = data.range;
                marker.shadows_enabled = data.shadows_enabled;
            }

            // Update the actual PointLight component
            if let Some(mut light) = world.get_mut::<PointLight>(entity) {
                light.color = new_color;
                light.intensity = data.intensity;
                light.range = data.range;
                light.shadows_enabled = data.shadows_enabled;
            }
        }
    }

    // Apply directional light changes
    if directional_light_changed {
        if let (Some(entity), Some(data)) = (single_entity, directional_light_data) {
            let new_color = Color::srgb(data.color[0], data.color[1], data.color[2]);

            // Update the marker component
            if let Some(mut marker) = world.get_mut::<DirectionalLightMarker>(entity) {
                marker.color = new_color;
                marker.illuminance = data.illuminance;
                marker.shadows_enabled = data.shadows_enabled;
            }

            // Update the actual DirectionalLight component
            if let Some(mut light) = world.get_mut::<DirectionalLight>(entity) {
                light.color = new_color;
                light.illuminance = data.illuminance;
                light.shadows_enabled = data.shadows_enabled;
            }
        }
    }

    // Update the panel state resource with the actual panel width
    if let Some(response) = &panel_response {
        if let Some(mut panel_state) = world.get_resource_mut::<InspectorPanelState>() {
            panel_state.width = response.response.rect.width();
        }
    }

    // Draw component browser window if open
    if let Some((entity, type_id)) = draw_component_browser(world, &ctx) {
        add_component_by_type_id(world, entity, type_id);
    }
}

/// Draw all components on an entity using reflection
fn draw_all_components(world: &mut World, entity: Entity, ui: &mut egui::Ui) {
    let type_registry = world.resource::<AppTypeRegistry>().clone();
    let type_registry_guard = type_registry.read();

    // Collect component type IDs for this entity
    let mut component_ids: Vec<(TypeId, String)> = {
        let entity_ref = world.entity(entity);
        let archetype = entity_ref.archetype();

        archetype
            .components()
            .iter()
            .filter_map(|&component_id| {
                let component_info = world.components().get_info(component_id)?;
                let type_id = component_info.type_id()?;

                // Check if this type is registered for reflection
                let registration = type_registry_guard.get(type_id)?;

                // Check if it has ReflectComponent
                if registration.data::<ReflectComponent>().is_none() {
                    return None;
                }

                let short_name = registration
                    .type_info()
                    .type_path_table()
                    .short_path()
                    .to_string();

                Some((type_id, short_name))
            })
            .collect()
    };

    // Sort components alphabetically by name
    component_ids.sort_by(|a, b| a.1.cmp(&b.1));

    drop(type_registry_guard);

    if component_ids.is_empty() {
        ui.label(
            egui::RichText::new("No reflectable components")
                .color(colors::TEXT_MUTED)
                .italics(),
        );
        return;
    }

    let config = ReflectEditorConfig::default();

    for (type_id, name) in component_ids {
        // Skip components we already have custom editors for
        if name == "Transform"
            || name == "SceneLightMarker"
            || name == "DirectionalLightMarker"
            || name == "RigidBody"
        {
            continue;
        }

        ui.add_space(2.0);

        // Draw the component using reflection
        component_editor(world, entity, type_id, ui, &config);
    }
}
