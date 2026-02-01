use avian3d::prelude::*;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::selection::Selected;

pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_inspector_panel);
    }
}

/// Draw the component inspector panel
fn draw_inspector_panel(
    mut contexts: EguiContexts,
    mut selected_query: Query<
        (
            Entity,
            Option<&Name>,
            Option<&mut Transform>,
            Option<&RigidBody>,
            Option<&Collider>,
        ),
        With<Selected>,
    >,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::SidePanel::right("inspector_panel")
        .default_width(300.0)
        .show(ctx, |ui| {
            ui.heading("Inspector");
            ui.separator();

            if let Ok((entity, name, transform, rigid_body, collider)) =
                selected_query.single_mut()
            {
                // Entity header
                let display_name: String = name
                    .map(|n: &Name| n.as_str().to_string())
                    .unwrap_or_else(|| format!("Entity {:?}", entity));
                ui.label(egui::RichText::new(&display_name).strong());
                ui.separator();

                // Transform component
                if let Some(mut transform) = transform {
                    ui.collapsing("Transform", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Position:");
                        });
                        ui.horizontal(|ui| {
                            ui.label("X:");
                            let mut x = transform.translation.x;
                            if ui
                                .add(egui::DragValue::new(&mut x).speed(0.1))
                                .changed()
                            {
                                transform.translation.x = x;
                            }
                            ui.label("Y:");
                            let mut y = transform.translation.y;
                            if ui
                                .add(egui::DragValue::new(&mut y).speed(0.1))
                                .changed()
                            {
                                transform.translation.y = y;
                            }
                            ui.label("Z:");
                            let mut z = transform.translation.z;
                            if ui
                                .add(egui::DragValue::new(&mut z).speed(0.1))
                                .changed()
                            {
                                transform.translation.z = z;
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Rotation (Euler):");
                        });
                        let (rx_rad, ry_rad, rz_rad) = transform.rotation.to_euler(EulerRot::XYZ);
                        let mut rx: f32 = rx_rad.to_degrees();
                        let mut ry: f32 = ry_rad.to_degrees();
                        let mut rz: f32 = rz_rad.to_degrees();
                        ui.horizontal(|ui| {
                            ui.label("X:");
                            let mut changed = false;
                            changed |= ui
                                .add(egui::DragValue::new(&mut rx).speed(1.0).suffix("deg"))
                                .changed();
                            ui.label("Y:");
                            changed |= ui
                                .add(egui::DragValue::new(&mut ry).speed(1.0).suffix("deg"))
                                .changed();
                            ui.label("Z:");
                            changed |= ui
                                .add(egui::DragValue::new(&mut rz).speed(1.0).suffix("deg"))
                                .changed();
                            if changed {
                                transform.rotation = Quat::from_euler(
                                    EulerRot::XYZ,
                                    rx.to_radians(),
                                    ry.to_radians(),
                                    rz.to_radians(),
                                );
                            }
                        });

                        ui.horizontal(|ui| {
                            ui.label("Scale:");
                        });
                        ui.horizontal(|ui| {
                            ui.label("X:");
                            let mut sx = transform.scale.x;
                            if ui
                                .add(egui::DragValue::new(&mut sx).speed(0.01).range(0.01..=100.0))
                                .changed()
                            {
                                transform.scale.x = sx;
                            }
                            ui.label("Y:");
                            let mut sy = transform.scale.y;
                            if ui
                                .add(egui::DragValue::new(&mut sy).speed(0.01).range(0.01..=100.0))
                                .changed()
                            {
                                transform.scale.y = sy;
                            }
                            ui.label("Z:");
                            let mut sz = transform.scale.z;
                            if ui
                                .add(egui::DragValue::new(&mut sz).speed(0.01).range(0.01..=100.0))
                                .changed()
                            {
                                transform.scale.z = sz;
                            }
                        });
                    });
                }

                // RigidBody component
                if let Some(rigid_body) = rigid_body {
                    ui.collapsing("RigidBody", |ui| {
                        let body_type = match rigid_body {
                            RigidBody::Dynamic => "Dynamic",
                            RigidBody::Static => "Static",
                            RigidBody::Kinematic => "Kinematic",
                        };
                        ui.label(format!("Type: {}", body_type));
                    });
                }

                // Collider component
                if let Some(_collider) = collider {
                    ui.collapsing("Collider", |ui| {
                        ui.label("Collider attached");
                    });
                }
            } else {
                ui.label("No entity selected");
                ui.label("");
                ui.label("Click an entity in the viewport");
                ui.label("or hierarchy to select it.");
            }
        });
    Ok(())
}
