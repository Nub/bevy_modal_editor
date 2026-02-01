use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::editor::{EditorMode, TransformOperation};
use crate::scene::{PrimitiveShape, SpawnPrimitiveEvent};
use super::{MarksWindowState, SettingsWindowState};

pub struct ToolbarPlugin;

impl Plugin for ToolbarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_toolbar);
    }
}

/// Draw the top toolbar
fn draw_toolbar(
    mut contexts: EguiContexts,
    mode: Res<State<EditorMode>>,
    mut next_mode: ResMut<NextState<EditorMode>>,
    mut transform_op: ResMut<TransformOperation>,
    mut spawn_events: MessageWriter<SpawnPrimitiveEvent>,
    mut settings_window: ResMut<SettingsWindowState>,
    mut marks_window: ResMut<MarksWindowState>,
) -> Result {
    let ctx = contexts.ctx_mut()?;

    egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            // Mode toggle buttons
            ui.label("Mode:");
            if ui
                .selectable_label(*mode.get() == EditorMode::View, "View")
                .clicked()
            {
                next_mode.set(EditorMode::View);
                *transform_op = TransformOperation::None;
            }
            if ui
                .selectable_label(*mode.get() == EditorMode::Edit, "Edit")
                .clicked()
            {
                next_mode.set(EditorMode::Edit);
            }

            ui.separator();

            // Transform operation buttons (only in Edit mode)
            if *mode.get() == EditorMode::Edit {
                ui.label("Tool:");
                if ui
                    .selectable_label(*transform_op == TransformOperation::Translate, "Move (Q)")
                    .clicked()
                {
                    *transform_op = TransformOperation::Translate;
                }
                if ui
                    .selectable_label(*transform_op == TransformOperation::Rotate, "Rotate (W)")
                    .clicked()
                {
                    *transform_op = TransformOperation::Rotate;
                }
                if ui
                    .selectable_label(*transform_op == TransformOperation::Scale, "Scale (E)")
                    .clicked()
                {
                    *transform_op = TransformOperation::Scale;
                }

                ui.separator();
            }

            // Primitive spawning
            ui.label("Add:");
            if ui.button("Cube").clicked() {
                spawn_events.write(SpawnPrimitiveEvent {
                    shape: PrimitiveShape::Cube,
                    position: Vec3::ZERO,
                });
            }
            if ui.button("Sphere").clicked() {
                spawn_events.write(SpawnPrimitiveEvent {
                    shape: PrimitiveShape::Sphere,
                    position: Vec3::ZERO,
                });
            }
            if ui.button("Cylinder").clicked() {
                spawn_events.write(SpawnPrimitiveEvent {
                    shape: PrimitiveShape::Cylinder,
                    position: Vec3::ZERO,
                });
            }
            if ui.button("Capsule").clicked() {
                spawn_events.write(SpawnPrimitiveEvent {
                    shape: PrimitiveShape::Capsule,
                    position: Vec3::ZERO,
                });
            }
            if ui.button("Plane").clicked() {
                spawn_events.write(SpawnPrimitiveEvent {
                    shape: PrimitiveShape::Plane,
                    position: Vec3::ZERO,
                });
            }

            ui.separator();

            // Scene operations
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Settings").clicked() {
                    settings_window.open = !settings_window.open;
                }
                if ui.button("Marks").clicked() {
                    marks_window.open = !marks_window.open;
                }
                ui.separator();
                if ui.button("Load").clicked() {
                    // TODO: Implement load dialog
                }
                if ui.button("Save").clicked() {
                    // TODO: Implement save dialog
                }
            });
        });
    });
    Ok(())
}
