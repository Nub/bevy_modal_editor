use avian3d::prelude::Physics;
use avian3d::schedule::PhysicsTime;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::commands::SnapshotHistory;
use crate::editor::{AxisConstraint, EditorMode, EditorState, SnapSubMode, TransformOperation};
use crate::scene::SceneFile;
use crate::ui::theme::{colors, popup_frame};
use crate::ui::Settings;

pub struct PanelsPlugin;

impl Plugin for PanelsPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, (draw_status_bar, draw_hint_bubble));
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
    snapshot_history: Res<SnapshotHistory>,
) -> Result {
    // Don't draw UI when editor is disabled
    if !editor_state.ui_enabled {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    egui::TopBottomPanel::bottom("status_bar")
        .frame(
            egui::Frame::side_top_panel(&ctx.style())
                .fill(colors::BG_DARK)
                .inner_margin(egui::Margin::symmetric(12, 6)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                // Mode indicator
                let mode_text = match mode.get() {
                    EditorMode::View => "VIEW",
                    EditorMode::Edit => "EDIT",
                    EditorMode::Insert => "INSERT",
                    EditorMode::ObjectInspector => "INSPECT",
                    EditorMode::Hierarchy => "HIERARCHY",
                };
                let mode_color = match mode.get() {
                    EditorMode::View => colors::ACCENT_BLUE,
                    EditorMode::Edit => colors::ACCENT_ORANGE,
                    EditorMode::Insert => colors::ACCENT_GREEN,
                    EditorMode::ObjectInspector => colors::ACCENT_PURPLE,
                    EditorMode::Hierarchy => colors::ACCENT_CYAN,
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
                        TransformOperation::SnapToObject => "Snap",
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

                ui.separator();

                // Undo/redo counts
                ui.label(
                    egui::RichText::new(format!(
                        "Undo: {} | Redo: {}",
                        snapshot_history.undo_count(),
                        snapshot_history.redo_count()
                    ))
                    .color(colors::TEXT_MUTED),
                );

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

/// Draw hint bubble with contextual hotkey guidance
fn draw_hint_bubble(
    mut contexts: EguiContexts,
    mode: Res<State<EditorMode>>,
    editor_state: Res<EditorState>,
    transform_op: Res<TransformOperation>,
    snap_submode: Res<SnapSubMode>,
    settings: Res<Settings>,
) -> Result {
    // Don't draw UI when editor is disabled or hints are off
    if !editor_state.ui_enabled || !settings.show_hints {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    // Get hints based on current mode and state
    let hints = get_hints_for_mode(*mode.get(), *transform_op, *snap_submode);

    if hints.is_empty() {
        return Ok(());
    }

    // Position above the status bar, centered horizontally
    egui::Area::new(egui::Id::new("hint_bubble"))
        .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -35.0])
        .show(ctx, |ui| {
            popup_frame(&ctx.style())
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, (key, desc)) in hints.iter().enumerate() {
                            if i > 0 {
                                ui.label(egui::RichText::new("│").color(colors::TEXT_MUTED));
                            }
                            ui.label(
                                egui::RichText::new(*key)
                                    .strong()
                                    .color(colors::ACCENT_ORANGE),
                            );
                            ui.label(
                                egui::RichText::new(*desc)
                                    .color(colors::TEXT_SECONDARY),
                            );
                        }
                    });
                });
        });

    Ok(())
}

/// Get contextual hints based on current mode and state
fn get_hints_for_mode(
    mode: EditorMode,
    transform_op: TransformOperation,
    snap_submode: SnapSubMode,
) -> Vec<(&'static str, &'static str)> {
    match mode {
        EditorMode::View => vec![
            ("V", "Edit"),
            ("I", "Insert"),
            ("O", "Inspect"),
            ("H", "Hierarchy"),
            ("C", "Commands"),
            ("?", "Help"),
        ],
        EditorMode::Edit => {
            match transform_op {
                TransformOperation::None => vec![
                    ("Q", "Move"),
                    ("W", "Rotate"),
                    ("E", "Scale"),
                    ("R", "Place"),
                    ("T", "Snap"),
                    ("Esc", "View"),
                ],
                TransformOperation::Translate | TransformOperation::Rotate | TransformOperation::Scale => vec![
                    ("A/S/D", "X/Y/Z axis"),
                    ("J/K", "Step -/+"),
                    ("Drag", "Transform"),
                    ("Esc", "View"),
                ],
                TransformOperation::Place => vec![
                    ("Move", "Position"),
                    ("Click", "Confirm"),
                    ("Esc", "Cancel"),
                ],
                TransformOperation::SnapToObject => {
                    let submode_hint = match snap_submode {
                        SnapSubMode::Surface => ("A", "Surface"),
                        SnapSubMode::Center => ("S", "Center"),
                        SnapSubMode::Aligned => ("D", "Aligned"),
                    };
                    vec![
                        submode_hint,
                        ("A/S/D", "Mode"),
                        ("Click", "Confirm"),
                        ("Esc", "Cancel"),
                    ]
                }
            }
        }
        EditorMode::Insert => vec![
            ("Type", "Search"),
            ("Enter", "Select"),
            ("Click", "Place"),
            ("Shift+Click", "Multi"),
            ("Esc", "Cancel"),
        ],
        EditorMode::ObjectInspector => vec![
            ("/", "Search"),
            ("I", "Add component"),
            ("N", "Name field"),
            ("Esc", "View"),
        ],
        EditorMode::Hierarchy => vec![
            ("/", "Search"),
            ("F", "Filter"),
            ("Drag", "Reparent"),
            ("Esc", "View"),
        ],
    }
}
