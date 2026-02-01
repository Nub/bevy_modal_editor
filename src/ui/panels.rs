use avian3d::prelude::Physics;
use avian3d::schedule::PhysicsTime;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::editor::{AxisConstraint, EditorMode, EditorState, TransformOperation};
use crate::scene::SceneFile;

pub struct PanelsPlugin;

impl Plugin for PanelsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_status_bar);
    }
}

/// Draw status bar showing current mode and editor state
fn draw_status_bar(
    mut contexts: EguiContexts,
    mode: Res<State<EditorMode>>,
    editor_state: Res<EditorState>,
    transform_op: Res<TransformOperation>,
    axis_constraint: Res<AxisConstraint>,
    scene_file: Res<SceneFile>,
    physics_time: Res<Time<Physics>>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            // Mode indicator
            let mode_text = match mode.get() {
                EditorMode::View => "VIEW",
                EditorMode::Edit => "EDIT",
                EditorMode::Insert => "INSERT",
            };
            let mode_color = match mode.get() {
                EditorMode::View => egui::Color32::from_rgb(100, 149, 237),
                EditorMode::Edit => egui::Color32::from_rgb(255, 165, 0),
                EditorMode::Insert => egui::Color32::from_rgb(100, 200, 100),
            };
            ui.colored_label(mode_color, format!("[{}]", mode_text));

            // Show transform operation and axis constraint in Edit mode
            if *mode.get() == EditorMode::Edit {
                let op_text = match *transform_op {
                    TransformOperation::None => "",
                    TransformOperation::Translate => "Translate",
                    TransformOperation::Rotate => "Rotate",
                    TransformOperation::Scale => "Scale",
                };
                if !op_text.is_empty() {
                    ui.separator();
                    ui.label(op_text);

                    // Show axis constraint
                    let axis_text = match *axis_constraint {
                        AxisConstraint::None => "",
                        AxisConstraint::X => "X",
                        AxisConstraint::Y => "Y",
                        AxisConstraint::Z => "Z",
                    };
                    if !axis_text.is_empty() {
                        let axis_color = match *axis_constraint {
                            AxisConstraint::X => egui::Color32::from_rgb(230, 80, 80),
                            AxisConstraint::Y => egui::Color32::from_rgb(80, 200, 80),
                            AxisConstraint::Z => egui::Color32::from_rgb(80, 130, 230),
                            AxisConstraint::None => egui::Color32::WHITE,
                        };
                        ui.colored_label(axis_color, format!("[{}]", axis_text));
                    }
                }
            }

            ui.separator();

            // Grid snap
            if editor_state.grid_snap > 0.0 {
                ui.label(format!("Grid: {:.2}", editor_state.grid_snap));
            } else {
                ui.label("Grid: Off");
            }

            ui.separator();

            // Rotation snap
            if editor_state.rotation_snap > 0.0 {
                ui.label(format!("Rot: {:.0}deg", editor_state.rotation_snap));
            } else {
                ui.label("Rot: Off");
            }

            ui.separator();

            // Physics status
            if physics_time.is_paused() {
                ui.colored_label(
                    egui::Color32::from_rgb(200, 100, 100),
                    "Physics: OFF",
                );
            } else {
                ui.colored_label(
                    egui::Color32::from_rgb(100, 200, 100),
                    "Physics: ON",
                );
            }

            // Right-justified file info
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Modified indicator (appears first due to RTL layout)
                if scene_file.modified {
                    ui.label(
                        egui::RichText::new("●")
                            .size(12.0)
                            .color(egui::Color32::from_rgb(255, 180, 100)),
                    );
                }

                // File name
                let file_name = scene_file.display_name();
                ui.label(
                    egui::RichText::new(file_name)
                        .size(13.0)
                        .color(egui::Color32::from_rgb(180, 180, 180)),
                );

                // File icon
                ui.label(egui::RichText::new("📄").size(13.0));
            });
        });
    });
    Ok(())
}
