//! AI mode panel for navmesh generation and visualization controls.

use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};

use crate::editor::{EditorMode, EditorState, PanelSide, PinnedWindows};
use crate::navigation::{GenerateNavmeshEvent, NavmeshState};
use crate::ui::theme::{colors, draw_pin_button, grid_label, panel, panel_frame, section_header, value_slider};

pub struct AIEditorPlugin;

impl Plugin for AIEditorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_ai_panel);
    }
}

/// Draw the AI panel (exclusive world access).
fn draw_ai_panel(world: &mut World) {
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    let current_mode = *world.resource::<State<EditorMode>>().get();
    let is_pinned = world.resource::<PinnedWindows>().0.contains(&EditorMode::AI);
    if current_mode != EditorMode::AI && !is_pinned {
        return;
    }

    // Clone state for mutation
    let mut nav_state = world.resource::<NavmeshState>().clone();
    let mut generate_requested = false;

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

    // Calculate panel position (right side)
    let panel_height = panel::available_height(&ctx);

    // If pinned and the active mode also uses the right side, move to the left
    let displaced = is_pinned
        && current_mode != EditorMode::AI
        && current_mode.panel_side() == Some(PanelSide::Right);
    let (anchor_align, anchor_offset) = if displaced {
        (egui::Align2::LEFT_TOP, [panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    } else {
        (egui::Align2::RIGHT_TOP, [-panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    };

    let mut pin_toggled = false;

    egui::Window::new("AI Navigation")
        .id(egui::Id::new("ai_editor_panel"))
        .frame(panel_frame(&ctx.style()))
        .anchor(anchor_align, anchor_offset)
        .default_width(panel::DEFAULT_WIDTH)
        .min_width(panel::MIN_WIDTH)
        .min_height(panel_height)
        .max_height(panel_height)
        .resizable(true)
        .collapsible(false)
        .title_bar(false)
        .show(&ctx, |ui| {
            // Title with pin button
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("AI")
                        .strong()
                        .color(colors::ACCENT_CYAN),
                );
                ui.label(
                    egui::RichText::new("Navigation")
                        .color(colors::TEXT_SECONDARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    pin_toggled = draw_pin_button(ui, is_pinned);
                });
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    // -- Navmesh section --
                    section_header(ui, "Navmesh", true, |ui| {
                        egui::Grid::new("navmesh_grid")
                            .num_columns(2)
                            .spacing([8.0, 4.0])
                            .show(ui, |ui| {
                                grid_label(ui, "Agent Radius");
                                value_slider(ui, &mut nav_state.agent_radius, 0.1..=2.0);
                                ui.end_row();

                                grid_label(ui, "Agent Height");
                                value_slider(ui, &mut nav_state.agent_height, 0.5..=5.0);
                                ui.end_row();
                            });

                        ui.add_space(4.0);

                        // Generate button
                        let button_text = if nav_state.generating {
                            "Generating..."
                        } else {
                            "Generate Navmesh"
                        };
                        let button = ui.add_enabled(
                            !nav_state.generating,
                            egui::Button::new(
                                egui::RichText::new(button_text).color(if nav_state.generating {
                                    colors::TEXT_MUTED
                                } else {
                                    colors::ACCENT_GREEN
                                }),
                            ),
                        );
                        if button.clicked() {
                            generate_requested = true;
                        }

                        ui.add_space(4.0);

                        // Status
                        if nav_state.ready {
                            ui.label(
                                egui::RichText::new(format!(
                                    "Ready ({} polygons)",
                                    nav_state.polygon_count
                                ))
                                .color(colors::STATUS_SUCCESS),
                            );
                        } else if nav_state.generating {
                            ui.label(
                                egui::RichText::new("Generating...")
                                    .color(colors::STATUS_WARNING),
                            );
                        } else {
                            ui.label(
                                egui::RichText::new("Not generated")
                                    .color(colors::TEXT_MUTED),
                            );
                        }
                    });

                    // -- Visualization section --
                    section_header(ui, "Visualization", true, |ui| {
                        ui.checkbox(&mut nav_state.show_wireframe, "Show navmesh wireframe");
                    });
                });
        });

    // Apply state changes back
    let mut state = world.resource_mut::<NavmeshState>();
    state.agent_radius = nav_state.agent_radius;
    state.agent_height = nav_state.agent_height;
    state.show_wireframe = nav_state.show_wireframe;

    if generate_requested {
        world.write_message(GenerateNavmeshEvent);
    }

    if pin_toggled {
        let mut pinned = world.resource_mut::<PinnedWindows>();
        if !pinned.0.remove(&EditorMode::AI) {
            pinned.0.insert(EditorMode::AI);
        }
    }
}
