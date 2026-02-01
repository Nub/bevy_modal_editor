use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::editor::{AxisConstraint, EditStepAmount, EditorMode, EditorState, SnapSubMode, TransformOperation};
use crate::ui::theme::colors;

pub struct EditInfoPlugin;

impl Plugin for EditInfoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_edit_info_window);
    }
}

/// Draw a small window showing edit info when in Edit mode
fn draw_edit_info_window(
    mut contexts: EguiContexts,
    mode: Res<State<EditorMode>>,
    transform_op: Res<TransformOperation>,
    axis_constraint: Res<AxisConstraint>,
    snap_submode: Res<SnapSubMode>,
    mut step_amount: ResMut<EditStepAmount>,
    mut editor_state: ResMut<EditorState>,
) -> Result {
    // Don't draw UI when editor is disabled
    if !editor_state.ui_enabled {
        return Ok(());
    }

    // Only show in Edit mode with an active operation
    if *mode.get() != EditorMode::Edit {
        return Ok(());
    }

    if *transform_op == TransformOperation::None {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    // Position inside viewport: offset from left to avoid hierarchy panel, from bottom to avoid status bar
    // Hierarchy panel is ~200px, status bar is ~25px
    let hierarchy_offset = 210.0;
    let status_bar_offset = 35.0;

    egui::Window::new("Edit")
        .resizable(false)
        .collapsible(false)
        .title_bar(false)
        .frame(egui::Frame::window(&ctx.style()).fill(colors::BG_DARK))
        .anchor(egui::Align2::LEFT_BOTTOM, [hierarchy_offset, -status_bar_offset])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Show current operation with color
                let (op_name, op_color) = match *transform_op {
                    TransformOperation::Translate => ("Move", colors::ACCENT_BLUE),
                    TransformOperation::Rotate => ("Rotate", colors::ACCENT_GREEN),
                    TransformOperation::Scale => ("Scale", colors::ACCENT_ORANGE),
                    TransformOperation::Place => ("Place", colors::ACCENT_PURPLE),
                    TransformOperation::SnapToObject => ("Snap", colors::ACCENT_PURPLE),
                    TransformOperation::None => ("", colors::TEXT_PRIMARY),
                };
                ui.label(egui::RichText::new(op_name).strong().color(op_color));

                ui.separator();

                // Show current axis with color
                let (axis_name, axis_color) = match *axis_constraint {
                    AxisConstraint::None => ("All", colors::TEXT_SECONDARY),
                    AxisConstraint::X => ("X", colors::AXIS_X),
                    AxisConstraint::Y => ("Y", colors::AXIS_Y),
                    AxisConstraint::Z => ("Z", colors::AXIS_Z),
                };
                ui.label(egui::RichText::new("Axis:").color(colors::TEXT_MUTED));
                ui.label(egui::RichText::new(axis_name).strong().color(axis_color));

                ui.separator();

                // Show and edit step amount
                ui.label(egui::RichText::new("Step:").color(colors::TEXT_MUTED));
                match *transform_op {
                    TransformOperation::Translate => {
                        ui.add(egui::DragValue::new(&mut step_amount.translate)
                            .speed(0.1)
                            .range(0.01..=10.0)
                            .suffix(" units"));
                    }
                    TransformOperation::Rotate => {
                        ui.add(egui::DragValue::new(&mut step_amount.rotate)
                            .speed(1.0)
                            .range(1.0..=90.0)
                            .suffix("°"));
                    }
                    TransformOperation::Scale => {
                        ui.add(egui::DragValue::new(&mut step_amount.scale)
                            .speed(0.01)
                            .range(0.01..=1.0));
                    }
                    TransformOperation::Place => {
                        ui.label(egui::RichText::new("Click to place").color(colors::TEXT_MUTED));
                    }
                    TransformOperation::SnapToObject => {
                        let submode_text = match *snap_submode {
                            SnapSubMode::Surface => "A:Surface",
                            SnapSubMode::Center => "S:Center",
                        };
                        ui.label(egui::RichText::new(submode_text).color(colors::ACCENT_BLUE));
                        ui.label(egui::RichText::new("Click to snap").color(colors::TEXT_MUTED));
                    }
                    TransformOperation::None => {}
                }
            });

            ui.add_space(4.0);

            // Snap controls
            ui.horizontal(|ui| {
                // Grid snap (for translate)
                let mut grid_enabled = editor_state.grid_snap > 0.0;
                if ui.checkbox(&mut grid_enabled, "Grid").changed() {
                    editor_state.grid_snap = if grid_enabled { 0.5 } else { 0.0 };
                }
                if grid_enabled {
                    ui.add(egui::DragValue::new(&mut editor_state.grid_snap)
                        .speed(0.1)
                        .range(0.1..=10.0));
                }

                ui.separator();

                // Rotation snap
                let mut rot_enabled = editor_state.rotation_snap > 0.0;
                if ui.checkbox(&mut rot_enabled, "Angle").changed() {
                    editor_state.rotation_snap = if rot_enabled { 15.0 } else { 0.0 };
                }
                if rot_enabled {
                    ui.add(egui::DragValue::new(&mut editor_state.rotation_snap)
                        .speed(1.0)
                        .range(1.0..=90.0)
                        .suffix("°"));
                }
            });

            ui.add_space(2.0);
            ui.label(
                egui::RichText::new("J/K: -/+ step | A/S/D: axis | Q/W/E/R: mode")
                    .small()
                    .color(colors::TEXT_MUTED)
            );
        });

    Ok(())
}
