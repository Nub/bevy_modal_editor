//! Right-side panel for the mesh modeling tool.
//!
//! Shows grid type selection, grid size controls, operation buttons,
//! extrude settings, and selected face count.

use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};

use crate::editor::{EditorMode, EditorState, PanelSide, PinnedWindows};
use crate::modeling::{GridType, MeshModelState, ModelOperation};
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

                // Grid Type section
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

                // Selected faces info
                if !state.selected_faces_empty {
                    ui.label(
                        egui::RichText::new(format!("{} faces selected", state.selected_faces_count))
                            .color(colors::ACCENT_GREEN),
                    );
                    ui.add_space(4.0);
                }

                // Operation section
                section_header(ui, "Operation", true, |ui| {
                    ui.horizontal(|ui| {
                        for op in [ModelOperation::Select, ModelOperation::Extrude, ModelOperation::Cut] {
                            let selected = state.pending_operation == op;
                            let label = op.display_name();
                            let text = if selected {
                                egui::RichText::new(label).strong().color(colors::ACCENT_ORANGE)
                            } else {
                                egui::RichText::new(label).color(colors::TEXT_SECONDARY)
                            };
                            if ui.selectable_label(selected, text).clicked() {
                                state.pending_operation = op;
                            }
                        }
                    });

                    // Extrude controls
                    if state.pending_operation == ModelOperation::Extrude {
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

                    // Apply button
                    if state.pending_operation != ModelOperation::Select && !state.selected_faces_empty {
                        ui.add_space(8.0);
                        let button_text = match state.pending_operation {
                            ModelOperation::Extrude => "Apply Extrude (Enter)",
                            ModelOperation::Cut => "Apply Cut (Enter)",
                            ModelOperation::Select => unreachable!(),
                        };
                        if ui.button(
                            egui::RichText::new(button_text).strong(),
                        ).clicked() {
                            state.confirm_requested = true;
                        }
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
    model_state.pending_operation = state.pending_operation;
    model_state.extrude_distance = state.extrude_distance;
    model_state.extrude_angle = state.extrude_angle;
    model_state.xray_selection = state.xray_selection;
}

/// Lightweight snapshot of MeshModelState for UI rendering.
/// Avoids borrowing the full resource (which contains `EditMesh`) through egui.
pub struct PanelSnapshot {
    pub grid_type: GridType,
    pub world_grid_size: f32,
    pub uv_grid_size: f32,
    pub surface_angle_threshold: f32,
    pub pending_operation: ModelOperation,
    pub extrude_distance: f32,
    pub extrude_angle: f32,
    pub target_entity: Option<Entity>,
    pub edit_mesh_face_count: Option<usize>,
    pub selected_faces_count: usize,
    pub selected_faces_empty: bool,
    pub confirm_requested: bool,
    pub xray_selection: bool,
}

impl MeshModelState {
    pub fn clone_state(&self) -> PanelSnapshot {
        PanelSnapshot {
            grid_type: self.grid_type,
            world_grid_size: self.world_grid_size,
            uv_grid_size: self.uv_grid_size,
            surface_angle_threshold: self.surface_angle_threshold,
            pending_operation: self.pending_operation,
            extrude_distance: self.extrude_distance,
            extrude_angle: self.extrude_angle,
            target_entity: self.target_entity,
            edit_mesh_face_count: self.edit_mesh.as_ref().map(|m| m.face_count()),
            selected_faces_count: self.selected_faces.len(),
            selected_faces_empty: self.selected_faces.is_empty(),
            confirm_requested: false,
            xray_selection: self.xray_selection,
        }
    }
}
