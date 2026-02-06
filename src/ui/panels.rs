use avian3d::prelude::Physics;
use avian3d::schedule::PhysicsTime;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::commands::SnapshotHistory;
use bevy_editor_game::GameState;

use crate::editor::{AxisConstraint, EditorCamera, EditorMode, EditorState, FlyCamera, SnapSubMode, TransformOperation};
use crate::scene::SceneFile;
use crate::selection::Selected;
use crate::ui::hierarchy::icons;
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
    sim_state: Res<State<GameState>>,
    selected_query: Query<&GlobalTransform, With<Selected>>,
    camera_query: Query<&FlyCamera, With<EditorCamera>>,
) -> Result {
    // Don't draw UI when editor is disabled
    if !editor_state.ui_enabled {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    // Get screen width for centering calculation
    let screen_width = ctx.input(|i| i.viewport_rect().width());

    egui::TopBottomPanel::bottom("status_bar")
        .frame(
            egui::Frame::side_top_panel(&ctx.style())
                .fill(colors::BG_DARK)
                .inner_margin(egui::Margin::symmetric(12, 6)),
        )
        .show(ctx, |ui| {
            // Draw file name centered using an overlay
            let file_name = scene_file.display_name();
            let status_bar_rect = ui.max_rect();
            let center_x = screen_width / 2.0;

            // Create text for centering calculation
            let file_text = if scene_file.modified {
                format!("{} {} {}", icons::FILE, file_name, icons::DOT)
            } else {
                format!("{} {}", icons::FILE, file_name)
            };

            // Draw centered file name using painter
            let painter = ui.painter();
            let galley = painter.layout_no_wrap(
                file_text.clone(),
                egui::FontId::default(),
                colors::TEXT_SECONDARY,
            );
            let text_width = galley.rect.width();
            let text_pos = egui::pos2(center_x - text_width / 2.0, status_bar_rect.center().y - galley.rect.height() / 2.0);
            painter.galley(text_pos, galley, colors::TEXT_SECONDARY);

            // Draw modified indicator if needed (colored)
            if scene_file.modified {
                let mod_galley = painter.layout_no_wrap(
                    icons::DOT.to_string(),
                    egui::FontId::proportional(12.0),
                    colors::STATUS_WARNING,
                );
                let mod_pos = egui::pos2(
                    center_x + text_width / 2.0 - mod_galley.rect.width() - 2.0,
                    status_bar_rect.center().y - mod_galley.rect.height() / 2.0,
                );
                painter.galley(mod_pos, mod_galley, colors::STATUS_WARNING);
            }

            ui.horizontal(|ui| {
                // LEFT: Mode, operation, FOV/Ortho, distance
                // Mode indicator
                let mode_text = match mode.get() {
                    EditorMode::View => "VIEW",
                    EditorMode::Edit => "EDIT",
                    EditorMode::Insert => "INSERT",
                    EditorMode::ObjectInspector => "INSPECT",
                    EditorMode::Hierarchy => "HIERARCHY",
                    EditorMode::Blockout => "BLOCKOUT",
                    EditorMode::Material => "MATERIAL",
                    EditorMode::Camera => "CAMERA",
                };
                let mode_color = match mode.get() {
                    EditorMode::View => colors::ACCENT_BLUE,
                    EditorMode::Edit => colors::ACCENT_ORANGE,
                    EditorMode::Insert => colors::ACCENT_GREEN,
                    EditorMode::ObjectInspector => colors::ACCENT_PURPLE,
                    EditorMode::Hierarchy => colors::ACCENT_CYAN,
                    EditorMode::Blockout => colors::ACCENT_ORANGE,
                    EditorMode::Material => colors::ACCENT_PURPLE,
                    EditorMode::Camera => colors::ACCENT_CYAN,
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

                // FOV / Ortho scale
                if let Ok(fly_cam) = camera_query.single() {
                    let cam_text = if fly_cam.fov_degrees == 0.0 {
                        format!("Ortho: {:.3}", fly_cam.ortho_scale)
                    } else {
                        format!("FOV: {:.0}°", fly_cam.fov_degrees)
                    };
                    ui.label(egui::RichText::new(cam_text).color(colors::TEXT_MUTED));
                }

                // Distance measurement (View mode only, when enabled and 2+ objects selected)
                if *mode.get() == EditorMode::View && editor_state.measurements_visible {
                    let positions: Vec<Vec3> = selected_query.iter().map(|t| t.translation()).collect();
                    if positions.len() >= 2 {
                        ui.separator();
                        let total_distance: f32 = positions
                            .windows(2)
                            .map(|w| w[0].distance(w[1]))
                            .sum();
                        ui.label(
                            egui::RichText::new(format!("{} {:.2}", icons::RULER, total_distance))
                                .color(colors::ACCENT_CYAN),
                        );
                    }
                }

                // Simulation state indicator (when not Editing)
                match sim_state.get() {
                    GameState::Playing => {
                        ui.separator();
                        ui.label(
                            egui::RichText::new("PLAYING")
                                .strong()
                                .color(colors::STATUS_SUCCESS),
                        );
                    }
                    GameState::Paused => {
                        ui.separator();
                        ui.label(
                            egui::RichText::new("PAUSED")
                                .strong()
                                .color(colors::STATUS_WARNING),
                        );
                    }
                    GameState::Editing => {}
                }

                // RIGHT: Physics + Undo/Redo (right-aligned)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Undo/redo counts (appears first due to RTL layout)
                    ui.label(
                        egui::RichText::new(format!(
                            "Undo: {} | Redo: {}",
                            snapshot_history.undo_count(),
                            snapshot_history.redo_count()
                        ))
                        .color(colors::TEXT_MUTED),
                    );

                    ui.separator();

                    // Physics status
                    if physics_time.relative_speed() == 0.0 {
                        ui.label(
                            egui::RichText::new("Physics: OFF")
                                .color(colors::STATUS_ERROR),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("Physics: ON")
                                .color(colors::STATUS_SUCCESS),
                        );
                    }
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
            ("E", "Edit"),
            ("I", "Insert"),
            ("O", "Inspect"),
            ("M", "Material"),
            ("V", "Camera"),
            ("H", "Hierarchy"),
            ("B", "Blockout"),
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
                TransformOperation::Translate => vec![
                    ("A/S/D", "X/Y/Z axis"),
                    ("J/K", "Step -/+"),
                    ("Alt+Drag", "Edge snap"),
                    ("Esc", "View"),
                ],
                TransformOperation::Rotate | TransformOperation::Scale => vec![
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
                        SnapSubMode::Surface => (icons::DOT, "Surface"),
                        SnapSubMode::Center => (icons::DOT, "Center"),
                        SnapSubMode::Aligned => (icons::DOT, "Aligned"),
                        SnapSubMode::Vertex => (icons::DOT, "Vertex"),
                    };
                    vec![
                        submode_hint,
                        ("Scroll", "Mode"),
                        ("Click", "Confirm"),
                        ("Esc", "Cancel"),
                    ]
                }
            }
        }
        EditorMode::Insert => {
            let submode_hint = match snap_submode {
                SnapSubMode::Surface => (icons::DOT, "Surface"),
                SnapSubMode::Center => (icons::DOT, "Center"),
                SnapSubMode::Aligned => (icons::DOT, "Aligned"),
                SnapSubMode::Vertex => (icons::DOT, "Vertex"),
            };
            vec![
                submode_hint,
                ("Scroll", "Mode"),
                ("Click", "Place"),
                ("Shift+Click", "Multi"),
                ("Esc", "Cancel"),
            ]
        }
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
        EditorMode::Blockout => vec![
            ("1-5", "Shape"),
            ("WASDQE", "Face"),
            ("R", "Rotate"),
            ("Enter", "Place"),
            ("Esc", "View"),
        ],
        EditorMode::Material => vec![
            ("F", "Find preset"),
            ("Drag", "Adjust values"),
            ("Esc", "View"),
        ],
        EditorMode::Camera => vec![
            ("P", "Preview"),
            ("R", "Revert"),
            ("Drag", "Adjust values"),
            ("Esc", "View"),
        ],
    }
}

