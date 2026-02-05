use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};
use bevy_editor_game::{ValidationMessage, ValidationRegistry, ValidationSeverity};

use crate::editor::EditorState;
use crate::selection::Selected;
use crate::ui::theme::colors;

/// Cached validation results, refreshed periodically.
#[derive(Resource, Default)]
pub struct ValidationResults {
    /// (rule_name, messages) pairs from the last validation run.
    pub results: Vec<(String, Vec<ValidationMessage>)>,
    /// Internal frame counter for throttling.
    pub frames_since_check: u32,
    /// Whether the popover is open.
    pub popover_open: bool,
}

impl ValidationResults {
    /// Total number of messages across all rules.
    pub fn total_count(&self) -> usize {
        self.results.iter().map(|(_, msgs)| msgs.len()).sum()
    }

    /// Count of messages at a given severity.
    pub fn count_by_severity(&self, severity: ValidationSeverity) -> usize {
        self.results
            .iter()
            .flat_map(|(_, msgs)| msgs.iter())
            .filter(|m| m.severity == severity)
            .count()
    }
}

pub struct ValidationPlugin;

impl Plugin for ValidationPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ValidationResults>()
            .add_systems(Update, run_validation)
            .add_systems(EguiPrimaryContextPass, draw_validation_panel);
    }
}

/// Run validation rules periodically (every 60 frames).
fn run_validation(world: &mut World) {
    let frames = {
        let mut res = world.resource_mut::<ValidationResults>();
        res.frames_since_check += 1;
        res.frames_since_check
    };

    if frames < 60 {
        return;
    }

    let Some(registry) = world.get_resource::<ValidationRegistry>() else {
        return;
    };

    if registry.rules.is_empty() {
        return;
    }

    // Collect rule info to avoid borrowing registry across validate calls
    let rules: Vec<(&'static str, fn(&mut World) -> Vec<ValidationMessage>)> = registry
        .rules
        .iter()
        .map(|r| (r.name, r.validate))
        .collect();

    let mut results = Vec::new();
    for (name, validate) in rules {
        let messages = validate(world);
        if !messages.is_empty() {
            results.push((name.to_string(), messages));
        }
    }

    let mut res = world.resource_mut::<ValidationResults>();
    res.results = results;
    res.frames_since_check = 0;
}

/// Draw the validation indicator in the status bar area.
fn draw_validation_panel(world: &mut World) {
    if !world.resource::<EditorState>().ui_enabled {
        return;
    }

    let total = world.resource::<ValidationResults>().total_count();
    if total == 0 {
        // Close popover if no messages
        world.resource_mut::<ValidationResults>().popover_open = false;
        return;
    }

    let error_count = world
        .resource::<ValidationResults>()
        .count_by_severity(ValidationSeverity::Error);
    let warning_count = world
        .resource::<ValidationResults>()
        .count_by_severity(ValidationSeverity::Warning);
    let info_count = world
        .resource::<ValidationResults>()
        .count_by_severity(ValidationSeverity::Info);

    let popover_open = world.resource::<ValidationResults>().popover_open;

    // Clone results for drawing
    let results: Vec<(String, Vec<ValidationMessage>)> = world
        .resource::<ValidationResults>()
        .results
        .clone();

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

    // Draw indicator as a small floating area near the bottom-right, above status bar
    let indicator_color = if error_count > 0 {
        colors::STATUS_ERROR
    } else if warning_count > 0 {
        colors::STATUS_WARNING
    } else {
        colors::ACCENT_BLUE
    };

    let mut toggle_popover = false;
    let mut select_entity: Option<Entity> = None;

    egui::Area::new(egui::Id::new("validation_indicator"))
        .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -35.0])
        .show(&ctx, |ui| {
            let mut parts = Vec::new();
            if error_count > 0 {
                parts.push(format!("{} err", error_count));
            }
            if warning_count > 0 {
                parts.push(format!("{} warn", warning_count));
            }
            if info_count > 0 {
                parts.push(format!("{} info", info_count));
            }
            let label_text = parts.join(", ");

            let button = ui.add(
                egui::Button::new(
                    egui::RichText::new(format!("! {}", label_text))
                        .small()
                        .color(indicator_color),
                )
                .frame(true),
            );
            if button.clicked() {
                toggle_popover = true;
            }
        });

    // Draw popover if open
    if popover_open {
        egui::Window::new("Validation")
            .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -60.0])
            .default_width(350.0)
            .max_height(300.0)
            .resizable(true)
            .collapsible(false)
            .show(&ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (rule_name, messages) in &results {
                        ui.label(
                            egui::RichText::new(rule_name.as_str())
                                .strong()
                                .color(colors::TEXT_PRIMARY),
                        );
                        for msg in messages {
                            let severity_color = match msg.severity {
                                ValidationSeverity::Error => colors::STATUS_ERROR,
                                ValidationSeverity::Warning => colors::STATUS_WARNING,
                                ValidationSeverity::Info => colors::ACCENT_BLUE,
                            };
                            let severity_label = match msg.severity {
                                ValidationSeverity::Error => "ERR",
                                ValidationSeverity::Warning => "WARN",
                                ValidationSeverity::Info => "INFO",
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new(severity_label)
                                        .small()
                                        .color(severity_color),
                                );
                                let response = ui.label(
                                    egui::RichText::new(&msg.message)
                                        .color(colors::TEXT_SECONDARY),
                                );
                                if let Some(entity) = msg.entity {
                                    if response.on_hover_text("Click to select entity").clicked() {
                                        select_entity = Some(entity);
                                    }
                                }
                            });
                        }
                        ui.add_space(4.0);
                    }
                });
            });
    }

    // Apply state changes
    if toggle_popover {
        world.resource_mut::<ValidationResults>().popover_open = !popover_open;
    }

    // Select entity if clicked in validation panel
    if let Some(target) = select_entity {
        // Deselect all
        let selected: Vec<Entity> = {
            let mut q = world.query_filtered::<Entity, With<Selected>>();
            q.iter(world).collect()
        };
        for e in selected {
            if let Ok(mut entity_mut) = world.get_entity_mut(e) {
                entity_mut.remove::<Selected>();
            }
        }
        // Select the target
        if let Ok(mut entity_mut) = world.get_entity_mut(target) {
            entity_mut.insert(Selected);
        }
    }
}
