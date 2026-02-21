use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use super::InspectorPanelState;
use crate::editor::{AxisConstraint, CameraPreset, EditorCamera, EditorMode, EditorState, FlyCamera, SetCameraPresetEvent};
use crate::ui::theme::colors;

pub struct ViewGizmoPlugin;

impl Plugin for ViewGizmoPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_view_gizmo);
    }
}

/// Draw a 3D orientation gizmo in the top-right corner
fn draw_view_gizmo(
    mut contexts: EguiContexts,
    camera_query: Query<(&FlyCamera, &GlobalTransform), With<EditorCamera>>,
    mut preset_events: MessageWriter<SetCameraPresetEvent>,
    mode: Res<State<EditorMode>>,
    mut axis_constraint: ResMut<AxisConstraint>,
    inspector_panel: Res<InspectorPanelState>,
    editor_state: Res<EditorState>,
) -> Result {
    // Don't draw UI when editor is disabled
    if !editor_state.ui_enabled {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let Ok((fly_cam, camera_transform)) = camera_query.single() else {
        return Ok(());
    };

    // Get the camera's rotation to transform the gizmo axes
    let camera_rotation = camera_transform.to_scale_rotation_translation().1;
    // We want the inverse rotation to show world axes from camera's perspective
    let view_rotation = camera_rotation.inverse();

    // Gizmo configuration
    let gizmo_size = 80.0;
    let gizmo_margin = 10.0;
    let axis_length = 30.0;
    let sphere_radius = 8.0;

    // Position in top-right corner of the viewport (not the inspector panel)
    // Account for the right inspector panel width dynamically
    let screen_rect = ctx.input(|i| i.viewport_rect());
    let inspector_panel_width = if inspector_panel.width > 0.0 {
        inspector_panel.width
    } else {
        300.0 // Default fallback on first frame
    };

    // Create a floating area for the gizmo
    egui::Area::new(egui::Id::new("view_gizmo"))
        .fixed_pos(egui::pos2(
            screen_rect.max.x - inspector_panel_width - gizmo_margin - gizmo_size,
            screen_rect.min.y + gizmo_margin + 30.0,
        ))
        .show(ctx, |ui| {
            let (response, painter) = ui.allocate_painter(
                egui::vec2(gizmo_size, gizmo_size),
                egui::Sense::click(),
            );

            let center = response.rect.center();

            // Background circle
            painter.circle_filled(center, gizmo_size / 2.0 - 2.0, egui::Color32::from_rgba_unmultiplied(40, 40, 40, 200));
            painter.circle_stroke(center, gizmo_size / 2.0 - 2.0, egui::Stroke::new(1.0, colors::TEXT_MUTED));

            // Define world axes with positive and negative labels
            let axes = [
                (Vec3::X, colors::AXIS_X, "X", "-X", CameraPreset::Right, CameraPreset::Left, AxisConstraint::X),
                (Vec3::Y, colors::AXIS_Y, "Y", "-Y", CameraPreset::Top, CameraPreset::Bottom, AxisConstraint::Y),
                (Vec3::Z, colors::AXIS_Z, "Z", "-Z", CameraPreset::Front, CameraPreset::Back, AxisConstraint::Z),
            ];

            // Current axis constraint for highlighting
            let current_constraint = *axis_constraint;

            // Store axis endpoints for click detection (sorted by depth for proper rendering)
            // (rotated_vec, pos_2d, color, label, preset, is_positive, axis_constraint)
            let mut axis_points: Vec<(Vec3, egui::Pos2, egui::Color32, &str, CameraPreset, bool, AxisConstraint)> = Vec::new();

            for (axis, color, pos_label, neg_label, pos_preset, neg_preset, constraint) in &axes {
                // Transform axis by view rotation
                let rotated_pos = view_rotation * *axis;
                let rotated_neg = view_rotation * (-*axis);

                // Project to 2D (simple orthographic projection)
                let pos_2d = egui::pos2(
                    center.x + rotated_pos.x * axis_length,
                    center.y - rotated_pos.y * axis_length, // flip Y for screen coords
                );
                let neg_2d = egui::pos2(
                    center.x + rotated_neg.x * axis_length,
                    center.y - rotated_neg.y * axis_length,
                );

                // Store with depth (z component) for sorting
                axis_points.push((rotated_pos, pos_2d, *color, *pos_label, *pos_preset, true, *constraint));
                axis_points.push((rotated_neg, neg_2d, color.gamma_multiply(0.5), *neg_label, *neg_preset, false, *constraint));
            }

            // Sort by depth (draw back-to-front)
            axis_points.sort_by(|a, b| a.0.z.partial_cmp(&b.0.z).unwrap());

            // Check if in edit mode
            let in_edit_mode = *mode.get() == EditorMode::Edit;

            // Draw axes and spheres
            for (rotated, pos_2d, color, label, _preset, is_positive, constraint) in &axis_points {
                // Check if this axis is the active constraint
                let is_active = current_constraint == *constraint;

                // Draw axis line
                let line_alpha = if rotated.z < 0.0 { 0.4 } else { 1.0 };
                let line_width = if is_active && in_edit_mode { 3.0 } else { 2.0 };
                let line_color = egui::Color32::from_rgba_unmultiplied(
                    color.r(),
                    color.g(),
                    color.b(),
                    (255.0 * line_alpha) as u8,
                );
                painter.line_segment([center, *pos_2d], egui::Stroke::new(line_width, line_color));

                // Draw sphere at axis end
                let base_sphere_size = if rotated.z < 0.0 { sphere_radius * 0.7 } else { sphere_radius };
                let sphere_size = if is_active && in_edit_mode { base_sphere_size * 1.3 } else { base_sphere_size };
                painter.circle_filled(*pos_2d, sphere_size, *color);

                // Draw highlight ring for active axis in edit mode
                if is_active && in_edit_mode {
                    painter.circle_stroke(*pos_2d, sphere_size + 2.0, egui::Stroke::new(2.0, colors::TEXT_PRIMARY));
                }

                // Draw label
                if *is_positive {
                    let font_size = if rotated.z < 0.0 { 12.0 } else { 14.0 };
                    let text_color = if rotated.z < 0.0 {
                        colors::TEXT_SECONDARY
                    } else {
                        colors::TEXT_PRIMARY
                    };
                    painter.text(
                        *pos_2d,
                        egui::Align2::CENTER_CENTER,
                        *label,
                        egui::FontId::proportional(font_size),
                        text_color,
                    );
                }
            }

            // Handle clicks on axis spheres
            if response.clicked() {
                if let Some(pointer_pos) = response.interact_pointer_pos() {
                    // Check which axis sphere was clicked (check front-to-back)
                    for (rotated, pos_2d, _, _, preset, _, constraint) in axis_points.iter().rev() {
                        let sphere_size = if rotated.z < 0.0 { sphere_radius * 0.7 } else { sphere_radius };
                        let dist = pointer_pos.distance(*pos_2d);
                        if dist <= sphere_size + 4.0 {
                            if in_edit_mode {
                                // In Edit mode: toggle axis constraint
                                if current_constraint == *constraint {
                                    *axis_constraint = AxisConstraint::None;
                                } else {
                                    *axis_constraint = *constraint;
                                }
                            } else {
                                // In View mode: set camera preset (keep current projection mode)
                                preset_events.write(SetCameraPresetEvent {
                                    preset: *preset,
                                    orthographic: false,
                                });
                            }
                            break;
                        }
                    }
                }
            }

            // Show current view info below gizmo
            let view_name = get_current_view_name(fly_cam);
            painter.text(
                egui::pos2(center.x, response.rect.max.y + 5.0),
                egui::Align2::CENTER_TOP,
                view_name,
                egui::FontId::proportional(12.0),
                colors::TEXT_SECONDARY,
            );
        });

    Ok(())
}

/// Determine the current view name based on camera angles
fn get_current_view_name(fly_cam: &FlyCamera) -> &'static str {
    use std::f32::consts::{FRAC_PI_2, PI};

    let yaw = fly_cam.yaw.rem_euclid(2.0 * PI);
    let pitch = fly_cam.pitch;

    const THRESHOLD: f32 = 0.1;

    // Check for top/bottom views first
    if (pitch - (FRAC_PI_2 - 0.001)).abs() < THRESHOLD {
        return "Top";
    }
    if (pitch - (-FRAC_PI_2 + 0.001)).abs() < THRESHOLD {
        return "Bottom";
    }

    // Check horizontal views
    if pitch.abs() < THRESHOLD {
        if yaw.abs() < THRESHOLD || (yaw - 2.0 * PI).abs() < THRESHOLD {
            return "Front";
        }
        if (yaw - PI).abs() < THRESHOLD {
            return "Back";
        }
        if (yaw - FRAC_PI_2).abs() < THRESHOLD {
            return "Left";
        }
        if (yaw - 3.0 * FRAC_PI_2).abs() < THRESHOLD || (yaw + FRAC_PI_2).abs() < THRESHOLD {
            return "Right";
        }
    }

    "Perspective"
}
