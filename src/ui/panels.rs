use avian3d::prelude::Physics;
use avian3d::schedule::PhysicsTime;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::editor::{AxisConstraint, EditorMode, EditorState, TransformOperation};
use crate::scene::SceneFile;
use crate::ui::theme::colors;

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

    egui::TopBottomPanel::bottom("status_bar")
        .frame(egui::Frame::side_top_panel(&ctx.style()).fill(colors::BG_DARK))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Mode indicator
                let mode_text = match mode.get() {
                    EditorMode::View => "VIEW",
                    EditorMode::Edit => "EDIT",
                    EditorMode::Insert => "INSERT",
                };
                let mode_color = match mode.get() {
                    EditorMode::View => colors::ACCENT_BLUE,
                    EditorMode::Edit => colors::ACCENT_ORANGE,
                    EditorMode::Insert => colors::ACCENT_GREEN,
                };
                ui.label(
                    egui::RichText::new(format!("[{}]", mode_text))
                        .strong()
                        .color(mode_color),
                );

                // Show transform operation and axis constraint in Edit mode
                if *mode.get() == EditorMode::Edit {
                    let op_text = match *transform_op {
                        TransformOperation::None => "",
                        TransformOperation::Translate => "Translate",
                        TransformOperation::Rotate => "Rotate",
                        TransformOperation::Scale => "Scale",
                        TransformOperation::Place => "Place",
                    };
                    if !op_text.is_empty() {
                        ui.separator();
                        ui.label(egui::RichText::new(op_text).color(colors::TEXT_SECONDARY));

                        // Show axis constraint
                        let axis_text = match *axis_constraint {
                            AxisConstraint::None => "",
                            AxisConstraint::X => "X",
                            AxisConstraint::Y => "Y",
                            AxisConstraint::Z => "Z",
                        };
                        if !axis_text.is_empty() {
                            let axis_color = match *axis_constraint {
                                AxisConstraint::X => colors::AXIS_X,
                                AxisConstraint::Y => colors::AXIS_Y,
                                AxisConstraint::Z => colors::AXIS_Z,
                                AxisConstraint::None => colors::TEXT_PRIMARY,
                            };
                            ui.label(
                                egui::RichText::new(format!("[{}]", axis_text))
                                    .strong()
                                    .color(axis_color),
                            );
                        }
                    }
                }

                ui.separator();

                // Grid snap
                let grid_text = if editor_state.grid_snap > 0.0 {
                    format!("Grid: {:.2}", editor_state.grid_snap)
                } else {
                    "Grid: Off".to_string()
                };
                ui.label(egui::RichText::new(grid_text).color(colors::TEXT_MUTED));

                ui.separator();

                // Rotation snap
                let rot_text = if editor_state.rotation_snap > 0.0 {
                    format!("Rot: {:.0}°", editor_state.rotation_snap)
                } else {
                    "Rot: Off".to_string()
                };
                ui.label(egui::RichText::new(rot_text).color(colors::TEXT_MUTED));

                ui.separator();

                // Physics status
                if physics_time.relative_speed() == 0.0 {
                    ui.label(
                        egui::RichText::new("▶ Physics: OFF")
                            .color(colors::STATUS_ERROR),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("▶ Physics: ON")
                            .color(colors::STATUS_SUCCESS),
                    );
                }

                // Right-justified file info
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Modified indicator (appears first due to RTL layout)
                    if scene_file.modified {
                        ui.label(
                            egui::RichText::new("●")
                                .size(12.0)
                                .color(colors::STATUS_WARNING),
                        );
                    }

                    // File name
                    let file_name = scene_file.display_name();
                    ui.label(
                        egui::RichText::new(file_name)
                            .color(colors::TEXT_SECONDARY),
                    );

                    // File icon
                    ui.label(egui::RichText::new("📄").size(14.0));
                });
            });
        });
    Ok(())
}
