use avian3d::prelude::RigidBody;
use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};
use bevy_inspector_egui::bevy_inspector::ui_for_entity;

use super::InspectorPanelState;
use crate::selection::Selected;
use crate::ui::theme::{colors, fonts};

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

/// Draw a transform section with colored X/Y/Z labels
fn draw_transform_section(ui: &mut egui::Ui, transform: &mut Transform) -> bool {
    let mut changed = false;

    egui::CollapsingHeader::new(
        egui::RichText::new("⊞ Transform").strong().color(colors::TEXT_PRIMARY),
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

/// Result of drawing a removable component section
enum ComponentAction<T> {
    None,
    Update(T),
    Remove,
}

/// Draw a RigidBody type selector with remove button
/// current_type is None if entities have mixed types
fn draw_rigidbody_section(ui: &mut egui::Ui, current_type: Option<RigidBodyType>) -> ComponentAction<RigidBodyType> {
    let mut action = ComponentAction::None;

    ui.horizontal(|ui| {
        ui.collapsing(
            egui::RichText::new("⚙ Physics").strong().color(colors::TEXT_PRIMARY),
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
            if ui.small_button(egui::RichText::new("✕").color(colors::TEXT_MUTED))
                .on_hover_text("Remove Physics component")
                .clicked()
            {
                action = ComponentAction::Remove;
            }
        });
    });

    action
}

/// Draw the component inspector panel using bevy-inspector-egui
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

    let panel_response = egui::SidePanel::right("inspector_panel")
        .default_width(300.0)
        .frame(
            egui::Frame::side_top_panel(&ctx.style())
                .fill(colors::PANEL_BG)
                .inner_margin(egui::Margin { left: 12, right: 8, top: 0, bottom: 8 })
        )
        .show(&ctx, |ui| {
            // Header
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("Inspector")
                        .strong()
                        .size(fonts::TITLE_SIZE)
                        .color(colors::TEXT_PRIMARY),
                );
            });
            ui.add_space(4.0);
            ui.separator();

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
                        ui.add(
                            egui::TextEdit::singleline(name)
                                .font(egui::FontId::proportional(16.0))
                                .text_color(colors::TEXT_PRIMARY)
                                .margin(egui::vec2(8.0, 6.0)),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new(format!("Entity {:?}", entity))
                                .strong()
                                .size(14.0)
                                .color(colors::TEXT_PRIMARY),
                        );
                    }
                    ui.label(
                        egui::RichText::new(format!("ID: {:?}", entity))
                            .small()
                            .color(colors::TEXT_MUTED),
                    );

                    ui.add_space(4.0);
                    ui.separator();
                    ui.add_space(4.0);

                    egui::ScrollArea::vertical().show(ui, |ui| {
                        // Custom Transform section with colored labels
                        if let Some(ref mut transform) = transform_copy {
                            transform_changed = draw_transform_section(ui, transform);
                        }

                        ui.add_space(4.0);

                        // RigidBody type selector (only if entity has RigidBody)
                        if has_rigidbodies {
                            rigidbody_action = draw_rigidbody_section(ui, common_rigidbody_type);
                            ui.add_space(4.0);
                        }

                        ui.separator();
                        ui.add_space(4.0);

                        // Other components via bevy-inspector-egui (hidden by default)
                        egui::CollapsingHeader::new(
                            egui::RichText::new("Show All Components").color(colors::TEXT_SECONDARY),
                        )
                        .default_open(false)
                        .show(ui, |ui| {
                            ui_for_entity(world, entity, ui);
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

    // Update the panel state resource with the actual panel width
    if let Some(mut panel_state) = world.get_resource_mut::<InspectorPanelState>() {
        panel_state.width = panel_response.response.rect.width();
    }
}
