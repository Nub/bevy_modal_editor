//! 2D UV editor panel.
//!
//! Shows a viewport into UV space with a grid background, UV wireframe
//! overlay of the edited mesh, seam edges highlighted, and selected
//! face UVs accented.

use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};

use crate::editor::{EditorMode, EditorState};
use crate::modeling::MeshModelState;

pub struct UvEditorPlugin;

impl Plugin for UvEditorPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UvEditorState>()
            .add_systems(EguiPrimaryContextPass, draw_uv_editor_panel);
    }
}

/// Persistent state for the UV editor panel.
#[derive(Resource)]
pub struct UvEditorState {
    /// Pan offset in UV space.
    pub pan: egui::Vec2,
    /// Zoom level (pixels per UV unit).
    pub zoom: f32,
}

impl Default for UvEditorState {
    fn default() -> Self {
        Self {
            pan: egui::Vec2::ZERO,
            zoom: 256.0,
        }
    }
}

const UV_BG: egui::Color32 = egui::Color32::from_rgb(30, 30, 35);
const UV_GRID_MAJOR: egui::Color32 = egui::Color32::from_rgb(60, 60, 70);
const UV_GRID_MINOR: egui::Color32 = egui::Color32::from_rgb(42, 42, 48);
const UV_BORDER: egui::Color32 = egui::Color32::from_rgb(100, 100, 120);
const UV_WIRE: egui::Color32 = egui::Color32::from_rgb(140, 140, 160);
const UV_SELECTED: egui::Color32 = egui::Color32::from_rgb(206, 145, 87);
const UV_SEAM: egui::Color32 = egui::Color32::from_rgb(220, 60, 60);

fn draw_uv_editor_panel(world: &mut World) {
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    let current_mode = *world.resource::<State<EditorMode>>().get();
    if current_mode != EditorMode::Blockout {
        return;
    }

    let model_state = world.resource::<MeshModelState>();
    if !model_state.show_uv_editor {
        return;
    }

    // Gather data we need from model state
    let edit_mesh = model_state.edit_mesh.clone();
    let selected_faces = model_state.selected_faces.clone();
    let seams = model_state.uv_seams.clone();

    let Some(edit_mesh) = edit_mesh else {
        return;
    };

    let uv_state = world.resource::<UvEditorState>();
    let pan = uv_state.pan;
    let zoom = uv_state.zoom;

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

    let mut new_pan = pan;
    let mut new_zoom = zoom;

    egui::Window::new("UV Editor")
        .default_size([400.0, 400.0])
        .resizable(true)
        .collapsible(true)
        .show(&ctx, |ui| {
            let (response, painter) =
                ui.allocate_painter(ui.available_size(), egui::Sense::click_and_drag());
            let rect = response.rect;

            // Background
            painter.rect_filled(rect, 0.0, UV_BG);

            // Handle pan (middle mouse or right drag)
            if response.dragged_by(egui::PointerButton::Middle)
                || response.dragged_by(egui::PointerButton::Secondary)
            {
                new_pan += response.drag_delta();
            }

            // Handle zoom (scroll wheel)
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll.abs() > 0.0 {
                let factor = 1.0 + scroll * 0.005;
                new_zoom = (new_zoom * factor).clamp(32.0, 2048.0);

                // Zoom toward cursor
                if let Some(hover) = response.hover_pos() {
                    let cursor_in_rect = hover - rect.left_top();
                    let old_uv = screen_to_uv(cursor_in_rect, pan, zoom);
                    let new_screen = uv_to_screen(old_uv, new_pan, new_zoom);
                    new_pan += cursor_in_rect - new_screen;
                }
            }

            // Helper to convert UV to screen position
            let to_screen = |uv: egui::Pos2| -> egui::Pos2 {
                let s = uv_to_screen(uv, new_pan, new_zoom);
                rect.left_top() + s
            };

            // Draw grid lines
            draw_grid(&painter, rect, new_pan, new_zoom);

            // Draw [0,1] UV space border
            let corners = [
                to_screen(egui::pos2(0.0, 0.0)),
                to_screen(egui::pos2(1.0, 0.0)),
                to_screen(egui::pos2(1.0, 1.0)),
                to_screen(egui::pos2(0.0, 1.0)),
            ];
            for i in 0..4 {
                painter.line_segment([corners[i], corners[(i + 1) % 4]], egui::Stroke::new(1.5, UV_BORDER));
            }

            // Draw all triangle edges in UV space (wireframe)
            for tri in &edit_mesh.triangles {
                for i in 0..3 {
                    let a = tri[i] as usize;
                    let b = tri[(i + 1) % 3] as usize;
                    if a < edit_mesh.uvs.len() && b < edit_mesh.uvs.len() {
                        let uv_a = edit_mesh.uvs[a];
                        let uv_b = edit_mesh.uvs[b];
                        let pa = to_screen(egui::pos2(uv_a.x, 1.0 - uv_a.y));
                        let pb = to_screen(egui::pos2(uv_b.x, 1.0 - uv_b.y));
                        painter.line_segment([pa, pb], egui::Stroke::new(0.5, UV_WIRE));
                    }
                }
            }

            // Highlight selected face UVs
            for &fi in &selected_faces {
                if fi < edit_mesh.triangles.len() {
                    let tri = &edit_mesh.triangles[fi];
                    for i in 0..3 {
                        let a = tri[i] as usize;
                        let b = tri[(i + 1) % 3] as usize;
                        if a < edit_mesh.uvs.len() && b < edit_mesh.uvs.len() {
                            let uv_a = edit_mesh.uvs[a];
                            let uv_b = edit_mesh.uvs[b];
                            let pa = to_screen(egui::pos2(uv_a.x, 1.0 - uv_a.y));
                            let pb = to_screen(egui::pos2(uv_b.x, 1.0 - uv_b.y));
                            painter.line_segment([pa, pb], egui::Stroke::new(1.5, UV_SELECTED));
                        }
                    }
                }
            }

            // Draw seam edges
            for &(a, b) in &seams {
                let a = a as usize;
                let b = b as usize;
                if a < edit_mesh.uvs.len() && b < edit_mesh.uvs.len() {
                    let uv_a = edit_mesh.uvs[a];
                    let uv_b = edit_mesh.uvs[b];
                    let pa = to_screen(egui::pos2(uv_a.x, 1.0 - uv_a.y));
                    let pb = to_screen(egui::pos2(uv_b.x, 1.0 - uv_b.y));
                    painter.line_segment([pa, pb], egui::Stroke::new(2.0, UV_SEAM));
                }
            }
        });

    // Write back pan/zoom state
    let mut uv_state = world.resource_mut::<UvEditorState>();
    uv_state.pan = new_pan;
    uv_state.zoom = new_zoom;
}

/// Convert UV coordinates to screen-space offset within the painter rect.
fn uv_to_screen(uv: egui::Pos2, pan: egui::Vec2, zoom: f32) -> egui::Vec2 {
    egui::vec2(uv.x * zoom + pan.x, uv.y * zoom + pan.y)
}

/// Convert screen-space offset to UV coordinates.
fn screen_to_uv(screen: egui::Vec2, pan: egui::Vec2, zoom: f32) -> egui::Pos2 {
    egui::pos2((screen.x - pan.x) / zoom, (screen.y - pan.y) / zoom)
}

/// Draw background grid lines at 0.25 and 0.5 intervals.
fn draw_grid(painter: &egui::Painter, rect: egui::Rect, pan: egui::Vec2, zoom: f32) {
    // Determine visible UV range
    let uv_min = screen_to_uv(egui::Vec2::ZERO, pan, zoom);
    let uv_max = screen_to_uv(rect.size().into(), pan, zoom);

    // Minor grid at 0.25 intervals
    let step = if zoom > 128.0 { 0.125 } else if zoom > 64.0 { 0.25 } else { 0.5 };

    let x_start = (uv_min.x / step).floor() as i32;
    let x_end = (uv_max.x / step).ceil() as i32;
    let y_start = (uv_min.y / step).floor() as i32;
    let y_end = (uv_max.y / step).ceil() as i32;

    for ix in x_start..=x_end {
        let x = ix as f32 * step;
        let is_major = (x * 4.0).round() as i32 % 4 == 0; // Every 1.0 is major
        let color = if is_major { UV_GRID_MAJOR } else { UV_GRID_MINOR };
        let sx = x * zoom + pan.x + rect.left();
        if sx >= rect.left() && sx <= rect.right() {
            painter.line_segment(
                [egui::pos2(sx, rect.top()), egui::pos2(sx, rect.bottom())],
                egui::Stroke::new(if is_major { 1.0 } else { 0.5 }, color),
            );
        }
    }

    for iy in y_start..=y_end {
        let y = iy as f32 * step;
        let is_major = (y * 4.0).round() as i32 % 4 == 0;
        let color = if is_major { UV_GRID_MAJOR } else { UV_GRID_MINOR };
        let sy = y * zoom + pan.y + rect.top();
        if sy >= rect.top() && sy <= rect.bottom() {
            painter.line_segment(
                [egui::pos2(rect.left(), sy), egui::pos2(rect.right(), sy)],
                egui::Stroke::new(if is_major { 1.0 } else { 0.5 }, color),
            );
        }
    }
}
