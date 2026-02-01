use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};

use crate::scene::SceneEntity;
use crate::selection::Selected;

pub struct HierarchyPlugin;

impl Plugin for HierarchyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(EguiPrimaryContextPass, draw_hierarchy_panel);
    }
}

/// Draw the scene hierarchy panel
fn draw_hierarchy_panel(
    mut contexts: EguiContexts,
    scene_entities: Query<(Entity, Option<&Name>, Option<&Children>), With<SceneEntity>>,
    selected: Query<Entity, With<Selected>>,
    mut commands: Commands,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let selected_entity = selected.single().ok();

    egui::SidePanel::left("hierarchy_panel")
        .default_width(200.0)
        .show(ctx, |ui| {
            ui.heading("Hierarchy");
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                for (entity, name, _children) in scene_entities.iter() {
                    let display_name = name
                        .map(|n: &Name| n.as_str().to_string())
                        .unwrap_or_else(|| format!("Entity {:?}", entity));

                    let is_selected = selected_entity == Some(entity);
                    let response = ui.selectable_label(is_selected, &display_name);

                    if response.clicked() {
                        // Clear previous selection
                        for selected_e in selected.iter() {
                            commands.entity(selected_e).remove::<Selected>();
                        }
                        // Select this entity
                        commands.entity(entity).insert(Selected);
                    }
                }
            });

            ui.separator();
            ui.label(format!("Total: {} entities", scene_entities.iter().count()));
        });
    Ok(())
}
