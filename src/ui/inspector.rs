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

/// Draw a RigidBody type selector, returns Some(new_type) if changed
fn draw_rigidbody_section(ui: &mut egui::Ui, current_type: RigidBodyType) -> Option<RigidBodyType> {
    let mut new_type = None;

    egui::CollapsingHeader::new(
        egui::RichText::new("⚙ Physics").strong().color(colors::TEXT_PRIMARY),
    )
    .default_open(true)
    .show(ui, |ui| {
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Body Type").color(colors::TEXT_SECONDARY));
        });

        egui::ComboBox::from_id_salt("rigidbody_type")
            .selected_text(current_type.label())
            .show_ui(ui, |ui| {
                for rb_type in RigidBodyType::ALL {
                    if ui.selectable_value(&mut new_type, Some(rb_type), rb_type.label()).clicked() {
                        // Only set if different from current
                        if rb_type != current_type {
                            new_type = Some(rb_type);
                        } else {
                            new_type = None;
                        }
                    }
                }
            });

        ui.add_space(4.0);
    });

    new_type
}

/// Draw the component inspector panel using bevy-inspector-egui
fn draw_inspector_panel(world: &mut World) {
    // Query for selected entity first
    let selected_entity = {
        let mut query = world.query_filtered::<Entity, With<Selected>>();
        query.iter(world).next()
    };

    // Get entity name and transform before borrowing for egui
    let mut entity_name = selected_entity.and_then(|e| {
        world.get::<Name>(e).map(|n| n.as_str().to_string())
    });

    let mut transform_copy = selected_entity.and_then(|e| world.get::<Transform>(e).copied());
    let original_name = entity_name.clone();

    // Get current RigidBody type if present
    let current_rigidbody_type = selected_entity.and_then(|e| {
        world.get::<RigidBody>(e).map(RigidBodyType::from_rigid_body)
    });

    // Get egui context - scope it so the borrow ends before we use world in the closure
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
    let mut new_rigidbody_type: Option<RigidBodyType> = None;

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

            if let Some(entity) = selected_entity {
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
                    if let Some(rb_type) = current_rigidbody_type {
                        new_rigidbody_type = draw_rigidbody_section(ui, rb_type);
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
            } else {
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
        });

    // Apply transform changes back to the entity
    if transform_changed {
        if let (Some(entity), Some(new_transform)) = (selected_entity, transform_copy) {
            if let Some(mut transform) = world.get_mut::<Transform>(entity) {
                *transform = new_transform;
            }
        }
    }

    // Apply name changes back to the entity
    if entity_name != original_name {
        if let (Some(entity), Some(new_name)) = (selected_entity, entity_name) {
            if let Some(mut name) = world.get_mut::<Name>(entity) {
                name.set(new_name);
            }
        }
    }

    // Apply RigidBody type change
    if let (Some(entity), Some(new_type)) = (selected_entity, new_rigidbody_type) {
        // Remove old RigidBody and insert new one
        world.entity_mut(entity).remove::<RigidBody>();
        world.entity_mut(entity).insert(new_type.to_rigid_body());
    }

    // Update the panel state resource with the actual panel width
    if let Some(mut panel_state) = world.get_resource_mut::<InspectorPanelState>() {
        panel_state.width = panel_response.response.rect.width();
    }
}
