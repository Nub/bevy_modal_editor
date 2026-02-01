use bevy::prelude::*;
use bevy_egui::{egui, EguiPrimaryContextPass};
use bevy_inspector_egui::bevy_inspector::ui_for_entity;

use super::InspectorPanelState;
use crate::selection::Selected;

pub struct InspectorPlugin;

impl Plugin for InspectorPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_inspector_panel);
    }
}

/// Draw the component inspector panel using bevy-inspector-egui
fn draw_inspector_panel(world: &mut World) {
    // Query for selected entity first
    let selected_entity = {
        let mut query = world.query_filtered::<Entity, With<Selected>>();
        query.iter(world).next()
    };

    // Get entity name before borrowing for egui
    let entity_name = selected_entity.and_then(|e| {
        world
            .get::<Name>(e)
            .map(|n| n.as_str().to_string())
    });

    // Get egui context - scope it so the borrow ends before we use world in the closure
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
    // egui_ctx is now dropped, world is no longer borrowed

    let panel_response = egui::SidePanel::right("inspector_panel")
        .default_width(300.0)
        .show(&ctx, |ui| {
            ui.heading("Inspector");
            ui.separator();

            if let Some(entity) = selected_entity {
                let display_name = entity_name.unwrap_or_else(|| format!("Entity {:?}", entity));
                ui.label(egui::RichText::new(&display_name).strong().size(16.0));
                ui.label(format!("ID: {:?}", entity));
                ui.separator();

                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui_for_entity(world, entity, ui);
                });
            } else {
                ui.label("No entity selected");
                ui.label("");
                ui.label("Click an entity in the viewport");
                ui.label("or hierarchy to select it.");
            }
        });

    // Update the panel state resource with the actual panel width
    if let Some(mut panel_state) = world.get_resource_mut::<InspectorPanelState>() {
        panel_state.width = panel_response.response.rect.width();
    }
}
