//! Right-side panel for the mesh modeling tool.
//!
//! Shows grid type selection, grid size controls, operation buttons,
//! extrude settings, and selected face count.

use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};

use crate::editor::{EditorMode, EditorState, PanelSide, PinnedWindows};
use crate::modeling::boolean::BooleanOp;
use crate::modeling::mirror::MirrorAxis;
use crate::modeling::snap::SnapMode;
use crate::modeling::soft_select::FalloffCurve;
use crate::modeling::uv_project::{ProjectionAxis, UvProjection};
use crate::modeling::{GridType, MeshModelState, ModelOperation, SelectionMode};
use crate::ui::theme::{colors, draw_pin_button, panel, panel_frame, section_header, value_slider};

pub struct MeshModelPanelPlugin;

impl Plugin for MeshModelPanelPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_mesh_model_panel);
    }
}

fn draw_mesh_model_panel(world: &mut World) {
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    let current_mode = *world.resource::<State<EditorMode>>().get();
    let is_pinned = world
        .resource::<PinnedWindows>()
        .0
        .contains(&EditorMode::Blockout);
    if current_mode != EditorMode::Blockout && !is_pinned {
        return;
    }

    // Clone lightweight state snapshot to avoid borrow conflicts
    let mut state = world.resource::<MeshModelState>().clone_state();

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

    let panel_height = panel::available_height(&ctx);

    // If pinned and displaced by another right-side panel, move left
    let displaced = is_pinned
        && current_mode != EditorMode::Blockout
        && current_mode.panel_side() == Some(PanelSide::Right);
    let (anchor_align, anchor_offset) = if displaced {
        (
            egui::Align2::LEFT_TOP,
            [panel::WINDOW_PADDING, panel::WINDOW_PADDING],
        )
    } else {
        (
            egui::Align2::RIGHT_TOP,
            [-panel::WINDOW_PADDING, panel::WINDOW_PADDING],
        )
    };

    let mut pin_toggled = false;

    egui::Window::new("Model")
        .anchor(anchor_align, anchor_offset)
        .fixed_size([panel::DEFAULT_WIDTH, panel_height])
        .title_bar(false)
        .frame(panel_frame(&ctx.style()))
        .show(&ctx, |ui| {
            // Title bar
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("MODEL")
                        .strong()
                        .color(colors::ACCENT_ORANGE),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    pin_toggled = draw_pin_button(ui, is_pinned);
                });
            });
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                // Target info
                if state.target_entity.is_some() {
                    let face_count = state
                        .edit_mesh_face_count
                        .map(|c| format!("{} faces", c))
                        .unwrap_or_else(|| "No mesh".to_string());
                    ui.label(
                        egui::RichText::new(face_count).color(colors::TEXT_SECONDARY),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("Select a mesh entity")
                            .italics()
                            .color(colors::TEXT_MUTED),
                    );
                }

                ui.add_space(8.0);

                // Selection Mode section
                section_header(ui, "Element Mode", true, |ui| {
                    ui.horizontal(|ui| {
                        for mode in [SelectionMode::Vertex, SelectionMode::Edge, SelectionMode::Face] {
                            let is_active = state.selection_mode == mode;
                            let label = format!("{} ({})", mode.display_name(), mode.key_hint());
                            let text = if is_active {
                                egui::RichText::new(label).strong().color(colors::ACCENT_BLUE)
                            } else {
                                egui::RichText::new(label).color(colors::TEXT_SECONDARY)
                            };
                            if ui.selectable_label(is_active, text).clicked() {
                                state.selection_mode = mode;
                            }
                        }
                    });
                });

                ui.add_space(4.0);

                // X-ray selection toggle
                ui.horizontal(|ui| {
                    ui.checkbox(&mut state.xray_selection, "");
                    ui.label(
                        egui::RichText::new("X-ray Selection (X)")
                            .color(if state.xray_selection {
                                colors::ACCENT_ORANGE
                            } else {
                                colors::TEXT_SECONDARY
                            }),
                    );
                });

                ui.add_space(4.0);

                // Grid Type section (only in Face mode)
                if state.selection_mode == SelectionMode::Face {
                    section_header(ui, "Selection Grid", true, |ui| {
                        ui.horizontal(|ui| {
                            for grid in [GridType::WorldSpace, GridType::SurfaceSpace, GridType::UVSpace, GridType::Freeform] {
                                let selected = state.grid_type == grid;
                                let label = grid.display_name();
                                let text = if selected {
                                    egui::RichText::new(label).strong().color(colors::ACCENT_BLUE)
                                } else {
                                    egui::RichText::new(label).color(colors::TEXT_SECONDARY)
                                };
                                if ui.selectable_label(selected, text).clicked() {
                                    state.grid_type = grid;
                                }
                            }
                        });

                        ui.add_space(4.0);

                        // Grid size controls (context-dependent)
                        match state.grid_type {
                            GridType::WorldSpace => {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Grid Size").color(colors::TEXT_SECONDARY));
                                    value_slider(ui, &mut state.world_grid_size, 0.125..=8.0);
                                });
                                ui.label(
                                    egui::RichText::new("Click grid cells to select regions")
                                        .small()
                                        .color(colors::TEXT_MUTED),
                                );
                            }
                            GridType::UVSpace => {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("UV Grid Size").color(colors::TEXT_SECONDARY));
                                    value_slider(ui, &mut state.uv_grid_size, 0.01..=1.0);
                                });
                            }
                            GridType::SurfaceSpace => {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("Angle").color(colors::TEXT_SECONDARY));
                                    value_slider(ui, &mut state.surface_angle_threshold, 1.0..=90.0);
                                    ui.label(egui::RichText::new("°").color(colors::TEXT_MUTED));
                                });
                            }
                            GridType::Freeform => {
                                ui.label(
                                    egui::RichText::new("Click to draw polygon, close near start")
                                        .small()
                                        .color(colors::TEXT_MUTED),
                                );
                            }
                        }
                    });

                    ui.add_space(4.0);
                }

                // Selection info
                let has_selection = match state.selection_mode {
                    SelectionMode::Face => !state.selected_faces_empty,
                    _ => !state.element_selection_empty,
                };
                if has_selection {
                    let count = match state.selection_mode {
                        SelectionMode::Face => state.selected_faces_count,
                        _ => state.element_selection_count,
                    };
                    let kind = state.selection_mode.display_name().to_lowercase();
                    let label = if count == 1 {
                        format!("1 {} selected", kind)
                    } else {
                        format!("{} {}s selected", count, kind)
                    };
                    ui.label(
                        egui::RichText::new(label).color(colors::ACCENT_GREEN),
                    );
                    ui.add_space(4.0);
                }

                // Operation section
                section_header(ui, "Operation", true, |ui| {
                    // Show different operations based on selection mode
                    let ops: &[ModelOperation] = match state.selection_mode {
                        SelectionMode::Face => &[
                            ModelOperation::Select,
                            ModelOperation::Extrude,
                            ModelOperation::Cut,
                            ModelOperation::Inset,
                            ModelOperation::PushPull,
                        ],
                        SelectionMode::Edge => &[
                            ModelOperation::Select,
                            ModelOperation::Bevel,
                            ModelOperation::Bridge,
                            ModelOperation::EdgeLoop,
                        ],
                        SelectionMode::Vertex => &[
                            ModelOperation::Select,
                            ModelOperation::Weld,
                        ],
                    };

                    ui.horizontal(|ui| {
                        for &op in ops {
                            let is_active = state.pending_operation == op;
                            let label = op.display_name();
                            let text = if is_active {
                                egui::RichText::new(label).strong().color(colors::ACCENT_ORANGE)
                            } else {
                                egui::RichText::new(label).color(colors::TEXT_SECONDARY)
                            };
                            if ui.selectable_label(is_active, text).clicked() {
                                state.pending_operation = op;
                            }
                        }
                    });

                    // Operation-specific controls
                    match state.pending_operation {
                        ModelOperation::Extrude => {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Distance").color(colors::TEXT_SECONDARY));
                                value_slider(ui, &mut state.extrude_distance, -5.0..=5.0);
                            });
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Angle").color(colors::TEXT_SECONDARY));
                                value_slider(ui, &mut state.extrude_angle, -90.0..=90.0);
                                ui.label(egui::RichText::new("°").color(colors::TEXT_MUTED));
                            });
                        }
                        ModelOperation::Inset => {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Distance").color(colors::TEXT_SECONDARY));
                                value_slider(ui, &mut state.inset_distance, 0.01..=0.99);
                            });
                        }
                        ModelOperation::Bevel => {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Width").color(colors::TEXT_SECONDARY));
                                value_slider(ui, &mut state.bevel_width, 0.01..=2.0);
                            });
                        }
                        ModelOperation::PushPull => {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Distance").color(colors::TEXT_SECONDARY));
                                value_slider(ui, &mut state.push_pull_distance, -5.0..=5.0);
                            });
                        }
                        ModelOperation::Weld => {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(egui::RichText::new("Threshold").color(colors::TEXT_SECONDARY));
                                value_slider(ui, &mut state.weld_threshold, 0.001..=1.0);
                            });
                        }
                        _ => {}
                    }

                    // Apply button (whole-mesh ops don't need a selection)
                    let can_apply = state.pending_operation != ModelOperation::Select
                        && (has_selection || state.pending_operation.is_whole_mesh_op());
                    if can_apply {
                        ui.add_space(8.0);
                        let button_text = format!("Apply {} (Enter)", state.pending_operation.display_name());
                        if ui.button(
                            egui::RichText::new(button_text).strong(),
                        ).clicked() {
                            state.confirm_requested = true;
                        }
                    }
                });

                ui.add_space(8.0);

                // Mesh Tools section (whole-mesh operations)
                section_header(ui, "Mesh Tools", true, |ui| {
                    // Mirror
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Mirror").color(colors::TEXT_SECONDARY)).clicked() {
                            state.pending_operation = ModelOperation::Mirror;
                            state.confirm_requested = true;
                        }
                        for axis in [MirrorAxis::X, MirrorAxis::Y, MirrorAxis::Z] {
                            let selected = state.mirror_axis == axis;
                            let text = if selected {
                                egui::RichText::new(axis.display_name()).strong().color(colors::ACCENT_BLUE)
                            } else {
                                egui::RichText::new(axis.display_name()).color(colors::TEXT_SECONDARY)
                            };
                            if ui.selectable_label(selected, text).clicked() {
                                state.mirror_axis = axis;
                            }
                        }
                    });

                    // Smooth
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Smooth").color(colors::TEXT_SECONDARY)).clicked() {
                            state.pending_operation = ModelOperation::Smooth;
                            state.confirm_requested = true;
                        }
                        value_slider(ui, &mut state.smooth_factor, 0.1..=1.0);
                    });

                    // Subdivide
                    if ui.button(egui::RichText::new("Subdivide").color(colors::TEXT_SECONDARY)).clicked() {
                        state.pending_operation = ModelOperation::Subdivide;
                        state.confirm_requested = true;
                    }

                    // Fill Holes
                    if ui.button(egui::RichText::new("Fill Holes").color(colors::TEXT_SECONDARY)).clicked() {
                        state.pending_operation = ModelOperation::FillHoles;
                        state.confirm_requested = true;
                    }

                    // Plane Cut
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Plane Cut").color(colors::TEXT_SECONDARY)).clicked() {
                            state.pending_operation = ModelOperation::PlaneCut;
                            state.confirm_requested = true;
                        }
                        for axis in [MirrorAxis::X, MirrorAxis::Y, MirrorAxis::Z] {
                            let selected = state.plane_cut_axis == axis;
                            let text = if selected {
                                egui::RichText::new(axis.display_name()).strong().color(colors::ACCENT_BLUE)
                            } else {
                                egui::RichText::new(axis.display_name()).color(colors::TEXT_SECONDARY)
                            };
                            if ui.selectable_label(selected, text).clicked() {
                                state.plane_cut_axis = axis;
                            }
                        }
                    });

                    // Simplify
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Simplify").color(colors::TEXT_SECONDARY)).clicked() {
                            state.pending_operation = ModelOperation::Simplify;
                            state.confirm_requested = true;
                        }
                        value_slider(ui, &mut state.simplify_ratio, 0.05..=0.95);
                    });

                    // Remesh
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Remesh").color(colors::TEXT_SECONDARY)).clicked() {
                            state.pending_operation = ModelOperation::Remesh;
                            state.confirm_requested = true;
                        }
                        value_slider(ui, &mut state.remesh_edge_length, 0.05..=2.0);
                    });
                });

                ui.add_space(8.0);

                // UV Tools section
                section_header(ui, "UV Tools", true, |ui| {
                    // UV Projection
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Projection").color(colors::TEXT_SECONDARY));
                        for proj in [UvProjection::Box, UvProjection::Planar, UvProjection::Cylindrical] {
                            let selected = state.uv_projection == proj;
                            let text = if selected {
                                egui::RichText::new(proj.display_name()).strong().color(colors::ACCENT_BLUE)
                            } else {
                                egui::RichText::new(proj.display_name()).color(colors::TEXT_SECONDARY)
                            };
                            if ui.selectable_label(selected, text).clicked() {
                                state.uv_projection = proj;
                            }
                        }
                    });

                    // Projection axis (for planar/cylindrical)
                    if state.uv_projection != UvProjection::Box {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Axis").color(colors::TEXT_SECONDARY));
                            for axis in [ProjectionAxis::X, ProjectionAxis::Y, ProjectionAxis::Z] {
                                let selected = state.uv_projection_axis == axis;
                                let text = if selected {
                                    egui::RichText::new(axis.display_name()).strong().color(colors::ACCENT_BLUE)
                                } else {
                                    egui::RichText::new(axis.display_name()).color(colors::TEXT_SECONDARY)
                                };
                                if ui.selectable_label(selected, text).clicked() {
                                    state.uv_projection_axis = axis;
                                }
                            }
                        });
                    }

                    // UV scale
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new("Scale").color(colors::TEXT_SECONDARY));
                        value_slider(ui, &mut state.uv_projection_scale, 0.1..=10.0);
                    });

                    // Apply UV Project button
                    if ui.button(egui::RichText::new("Apply UV Project").color(colors::TEXT_SECONDARY)).clicked() {
                        state.pending_operation = ModelOperation::UvProject;
                        state.confirm_requested = true;
                    }

                    ui.add_space(4.0);

                    // UV Unwrap button
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("UV Unwrap (Seams)").color(colors::TEXT_SECONDARY)).clicked() {
                            state.pending_operation = ModelOperation::UvUnwrap;
                            state.confirm_requested = true;
                        }
                    });

                    // Seam count info
                    if state.uv_seam_count > 0 {
                        ui.label(
                            egui::RichText::new(format!("{} seam edges (T to toggle)", state.uv_seam_count))
                                .small()
                                .color(colors::ACCENT_ORANGE),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("No seams — select edges & press T")
                                .small()
                                .color(colors::TEXT_MUTED),
                        );
                    }

                    ui.add_space(4.0);

                    // UV Editor toggle
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut state.show_uv_editor, "");
                        ui.label(
                            egui::RichText::new("UV Editor (V)")
                                .color(if state.show_uv_editor {
                                    colors::ACCENT_BLUE
                                } else {
                                    colors::TEXT_SECONDARY
                                }),
                        );
                    });
                });

                ui.add_space(8.0);

                // Normals section
                section_header(ui, "Normals", true, |ui| {
                    // Auto Smooth
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Auto Smooth").color(colors::TEXT_SECONDARY)).clicked() {
                            state.pending_operation = ModelOperation::AutoSmooth;
                            state.confirm_requested = true;
                        }
                        value_slider(ui, &mut state.auto_smooth_angle, 0.0..=180.0);
                        ui.label(egui::RichText::new("°").color(colors::TEXT_MUTED));
                    });

                    // Flat Normals
                    if ui.button(egui::RichText::new("Flat Normals").color(colors::TEXT_SECONDARY)).clicked() {
                        state.pending_operation = ModelOperation::FlatNormals;
                        state.confirm_requested = true;
                    }

                    // Hard edge info
                    if state.hard_edge_count > 0 {
                        ui.label(
                            egui::RichText::new(format!("{} hard edges (H to toggle)", state.hard_edge_count))
                                .small()
                                .color(colors::ACCENT_ORANGE),
                        );
                    } else {
                        ui.label(
                            egui::RichText::new("No hard edges — select edges & press H")
                                .small()
                                .color(colors::TEXT_MUTED),
                        );
                    }
                });

                ui.add_space(8.0);

                // Subdivision section
                section_header(ui, "Subdivision", true, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button(egui::RichText::new("Catmull-Clark").color(colors::TEXT_SECONDARY)).clicked() {
                            state.pending_operation = ModelOperation::CatmullClark;
                            state.confirm_requested = true;
                        }
                        if ui.button(egui::RichText::new("Midpoint").color(colors::TEXT_SECONDARY)).clicked() {
                            state.pending_operation = ModelOperation::Subdivide;
                            state.confirm_requested = true;
                        }
                    });
                });

                ui.add_space(8.0);

                // Snapping section
                section_header(ui, "Snapping", true, |ui| {
                    ui.horizontal(|ui| {
                        for mode in [SnapMode::None, SnapMode::Grid, SnapMode::Vertex, SnapMode::EdgeMidpoint] {
                            let is_active = state.snap_mode == mode;
                            let text = if is_active {
                                egui::RichText::new(mode.display_name()).strong().color(colors::ACCENT_BLUE)
                            } else {
                                egui::RichText::new(mode.display_name()).color(colors::TEXT_SECONDARY)
                            };
                            if ui.selectable_label(is_active, text).clicked() {
                                state.snap_mode = mode;
                            }
                        }
                    });

                    if state.snap_mode == SnapMode::Grid {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Grid Size").color(colors::TEXT_SECONDARY));
                            value_slider(ui, &mut state.snap_grid_size, 0.0625..=2.0);
                        });
                    }

                    // Snap to Grid button
                    if ui.button(egui::RichText::new("Snap to Grid").color(colors::TEXT_SECONDARY)).clicked() {
                        state.pending_operation = ModelOperation::SnapToGrid;
                        state.confirm_requested = true;
                    }
                });

                ui.add_space(8.0);

                // Soft Selection section
                section_header(ui, "Soft Selection", true, |ui| {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut state.soft_selection, "");
                        ui.label(
                            egui::RichText::new("Enabled")
                                .color(if state.soft_selection {
                                    colors::ACCENT_GREEN
                                } else {
                                    colors::TEXT_SECONDARY
                                }),
                        );
                    });

                    if state.soft_selection {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Radius").color(colors::TEXT_SECONDARY));
                            value_slider(ui, &mut state.soft_radius, 0.1..=10.0);
                        });

                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new("Falloff").color(colors::TEXT_SECONDARY));
                            for curve in [FalloffCurve::Linear, FalloffCurve::Smooth, FalloffCurve::Sharp, FalloffCurve::Root] {
                                let is_active = state.soft_falloff == curve;
                                let text = if is_active {
                                    egui::RichText::new(curve.display_name()).strong().color(colors::ACCENT_BLUE)
                                } else {
                                    egui::RichText::new(curve.display_name()).color(colors::TEXT_SECONDARY)
                                };
                                if ui.selectable_label(is_active, text).clicked() {
                                    state.soft_falloff = curve;
                                }
                            }
                        });
                    }
                });
            });
        });

    // Write back changed state
    if pin_toggled {
        let mut pinned = world.resource_mut::<PinnedWindows>();
        if pinned.0.contains(&EditorMode::Blockout) {
            pinned.0.remove(&EditorMode::Blockout);
        } else {
            pinned.0.insert(EditorMode::Blockout);
        }
    }

    // Apply state changes back to the resource
    let mut model_state = world.resource_mut::<MeshModelState>();
    model_state.grid_type = state.grid_type;
    model_state.world_grid_size = state.world_grid_size;
    model_state.uv_grid_size = state.uv_grid_size;
    model_state.surface_angle_threshold = state.surface_angle_threshold;
    model_state.selection_mode = state.selection_mode;
    model_state.pending_operation = state.pending_operation;
    model_state.extrude_distance = state.extrude_distance;
    model_state.extrude_angle = state.extrude_angle;
    model_state.inset_distance = state.inset_distance;
    model_state.bevel_width = state.bevel_width;
    model_state.push_pull_distance = state.push_pull_distance;
    model_state.weld_threshold = state.weld_threshold;
    model_state.xray_selection = state.xray_selection;
    model_state.mirror_axis = state.mirror_axis;
    model_state.smooth_factor = state.smooth_factor;
    model_state.simplify_ratio = state.simplify_ratio;
    model_state.remesh_edge_length = state.remesh_edge_length;
    model_state.plane_cut_axis = state.plane_cut_axis;
    model_state.uv_projection = state.uv_projection;
    model_state.uv_projection_axis = state.uv_projection_axis;
    model_state.uv_projection_scale = state.uv_projection_scale;
    model_state.show_uv_editor = state.show_uv_editor;
    model_state.auto_smooth_angle = state.auto_smooth_angle;
    model_state.snap_mode = state.snap_mode;
    model_state.snap_grid_size = state.snap_grid_size;
    model_state.soft_selection = state.soft_selection;
    model_state.soft_radius = state.soft_radius;
    model_state.soft_falloff = state.soft_falloff;
    if state.confirm_requested {
        model_state.confirm_requested = true;
    }
}

/// Lightweight snapshot of MeshModelState for UI rendering.
/// Avoids borrowing the full resource (which contains `EditMesh`) through egui.
pub struct PanelSnapshot {
    pub grid_type: GridType,
    pub world_grid_size: f32,
    pub uv_grid_size: f32,
    pub surface_angle_threshold: f32,
    pub selection_mode: SelectionMode,
    pub pending_operation: ModelOperation,
    pub extrude_distance: f32,
    pub extrude_angle: f32,
    pub inset_distance: f32,
    pub bevel_width: f32,
    pub push_pull_distance: f32,
    pub weld_threshold: f32,
    pub target_entity: Option<Entity>,
    pub edit_mesh_face_count: Option<usize>,
    pub selected_faces_count: usize,
    pub selected_faces_empty: bool,
    pub element_selection_count: usize,
    pub element_selection_empty: bool,
    pub confirm_requested: bool,
    pub xray_selection: bool,
    // Phase 3 fields
    pub mirror_axis: MirrorAxis,
    pub smooth_factor: f32,
    pub simplify_ratio: f32,
    pub remesh_edge_length: f32,
    pub plane_cut_axis: MirrorAxis,
    // Phase 4 fields
    pub uv_projection: UvProjection,
    pub uv_projection_axis: ProjectionAxis,
    pub uv_projection_scale: f32,
    pub uv_seam_count: usize,
    pub show_uv_editor: bool,
    // Phase 5 fields
    pub auto_smooth_angle: f32,
    pub hard_edge_count: usize,
    pub snap_mode: SnapMode,
    pub snap_grid_size: f32,
    pub soft_selection: bool,
    pub soft_radius: f32,
    pub soft_falloff: FalloffCurve,
}

impl MeshModelState {
    pub fn clone_state(&self) -> PanelSnapshot {
        PanelSnapshot {
            grid_type: self.grid_type,
            world_grid_size: self.world_grid_size,
            uv_grid_size: self.uv_grid_size,
            surface_angle_threshold: self.surface_angle_threshold,
            selection_mode: self.selection_mode,
            pending_operation: self.pending_operation,
            extrude_distance: self.extrude_distance,
            extrude_angle: self.extrude_angle,
            inset_distance: self.inset_distance,
            bevel_width: self.bevel_width,
            push_pull_distance: self.push_pull_distance,
            weld_threshold: self.weld_threshold,
            target_entity: self.target_entity,
            edit_mesh_face_count: self.edit_mesh.as_ref().map(|m| m.face_count()),
            selected_faces_count: self.selected_faces.len(),
            selected_faces_empty: self.selected_faces.is_empty(),
            element_selection_count: self.element_selection.count(),
            element_selection_empty: self.element_selection.is_empty(),
            confirm_requested: false,
            xray_selection: self.xray_selection,
            mirror_axis: self.mirror_axis,
            smooth_factor: self.smooth_factor,
            simplify_ratio: self.simplify_ratio,
            remesh_edge_length: self.remesh_edge_length,
            plane_cut_axis: self.plane_cut_axis,
            uv_projection: self.uv_projection,
            uv_projection_axis: self.uv_projection_axis,
            uv_projection_scale: self.uv_projection_scale,
            uv_seam_count: self.uv_seams.len(),
            show_uv_editor: self.show_uv_editor,
            auto_smooth_angle: self.auto_smooth_angle,
            hard_edge_count: self.hard_edges.len(),
            snap_mode: self.snap_mode,
            snap_grid_size: self.snap_grid_size,
            soft_selection: self.soft_selection,
            soft_radius: self.soft_radius,
            soft_falloff: self.soft_falloff,
        }
    }
}
