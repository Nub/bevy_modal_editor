use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::editor::{
    CameraMarks, JumpToLastPositionEvent, JumpToMarkEvent, SetCameraMarkEvent,
};

/// Resource to track if marks window is open
#[derive(Resource, Default)]
pub struct MarksWindowState {
    pub open: bool,
    /// Text input for new mark name
    pub new_mark_name: String,
}

pub struct MarksPlugin;

impl Plugin for MarksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MarksWindowState>()
            .add_systems(EguiPrimaryContextPass, draw_marks_window);
    }
}

/// Draw the camera marks window
fn draw_marks_window(
    mut contexts: EguiContexts,
    mut window_state: ResMut<MarksWindowState>,
    mut marks: ResMut<CameraMarks>,
    mut set_events: MessageWriter<SetCameraMarkEvent>,
    mut jump_events: MessageWriter<JumpToMarkEvent>,
    mut last_events: MessageWriter<JumpToLastPositionEvent>,
) -> Result {
    if !window_state.open {
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    let mut open = window_state.open;
    egui::Window::new("Camera Marks")
        .open(&mut open)
        .default_width(200.0)
        .resizable(true)
        .show(ctx, |ui| {
            // Quick jump to last position
            ui.horizontal(|ui| {
                if ui.button("Jump to Last (`)").clicked() {
                    last_events.write(JumpToLastPositionEvent);
                }
            });

            ui.separator();

            // Add new mark section
            ui.horizontal(|ui| {
                ui.label("New mark:");
                ui.text_edit_singleline(&mut window_state.new_mark_name);
                if ui.button("Set").clicked() && !window_state.new_mark_name.is_empty() {
                    set_events.write(SetCameraMarkEvent {
                        name: window_state.new_mark_name.clone(),
                    });
                    window_state.new_mark_name.clear();
                }
            });

            ui.separator();

            // Quick marks (1-9)
            ui.label("Quick marks (Shift+1-9 to set, 1-9 to jump):");
            egui::Grid::new("quick_marks_grid")
                .num_columns(3)
                .spacing([4.0, 4.0])
                .show(ui, |ui| {
                    for i in 1..=9 {
                        let name = i.to_string();
                        let has_mark = marks.marks.contains_key(&name);

                        if has_mark {
                            if ui.button(format!("[{}]", i)).clicked() {
                                jump_events.write(JumpToMarkEvent { name: name.clone() });
                            }
                        } else {
                            ui.add_enabled(false, egui::Button::new(format!(" {} ", i)));
                        }

                        if i % 3 == 0 {
                            ui.end_row();
                        }
                    }
                });

            ui.separator();

            // Named marks list
            ui.label("Named marks:");

            // Collect marks to display (excluding numeric quick marks)
            let named_marks: Vec<_> = marks
                .marks
                .iter()
                .filter(|(name, _)| name.parse::<u32>().is_err())
                .map(|(name, mark)| (name.clone(), mark.clone()))
                .collect();

            if named_marks.is_empty() {
                ui.label("(none)");
            } else {
                let mut to_delete: Option<String> = None;

                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        for (name, _mark) in &named_marks {
                            ui.horizontal(|ui| {
                                if ui.button(&**name).clicked() {
                                    jump_events.write(JumpToMarkEvent { name: name.clone() });
                                }
                                if ui.small_button("X").clicked() {
                                    to_delete = Some(name.clone());
                                }
                            });
                        }
                    });

                // Handle deletion outside the iteration
                if let Some(name) = to_delete {
                    marks.remove_mark(&name);
                }
            }

            ui.separator();

            // All marks count
            ui.label(format!("Total marks: {}", marks.marks.len()));
        });

    window_state.open = open;
    Ok(())
}
