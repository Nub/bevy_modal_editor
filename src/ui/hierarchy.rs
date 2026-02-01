use bevy::ecs::relationship::Relationship;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use std::collections::HashSet;

use crate::scene::{GroupMarker, SceneEntity};
use crate::selection::Selected;

pub struct HierarchyPlugin;

impl Plugin for HierarchyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HierarchyState>()
            .add_systems(EguiPrimaryContextPass, draw_hierarchy_panel);
    }
}

/// State for hierarchy panel (expanded nodes, etc.)
#[derive(Resource, Default)]
pub struct HierarchyState {
    /// Set of expanded group entities
    pub expanded: HashSet<Entity>,
}

/// Draw the scene hierarchy panel
fn draw_hierarchy_panel(
    mut contexts: EguiContexts,
    scene_entities: Query<
        (Entity, Option<&Name>, Option<&ChildOf>, Option<&Children>, Option<&GroupMarker>),
        With<SceneEntity>,
    >,
    selected: Query<Entity, With<Selected>>,
    mut commands: Commands,
    mut hierarchy_state: ResMut<HierarchyState>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    let selected_entity = selected.single().ok();

    egui::SidePanel::left("hierarchy_panel")
        .default_width(200.0)
        .show(ctx, |ui| {
            ui.heading("Hierarchy");
            ui.separator();

            egui::ScrollArea::vertical().show(ui, |ui| {
                // Find root entities (no parent or parent is not a SceneEntity)
                let root_entities: Vec<_> = scene_entities
                    .iter()
                    .filter(
                        |(_, _, parent, _, _): &(
                            Entity,
                            Option<&Name>,
                            Option<&ChildOf>,
                            Option<&Children>,
                            Option<&GroupMarker>,
                        )| {
                            parent.map_or(true, |p| scene_entities.get(p.get()).is_err())
                        },
                    )
                    .collect();

                for (entity, name, _, children, is_group) in root_entities {
                    draw_entity_row(
                        ui,
                        entity,
                        name,
                        children,
                        is_group.is_some(),
                        0,
                        selected_entity,
                        &scene_entities,
                        &mut commands,
                        &selected,
                        &mut hierarchy_state,
                    );
                }
            });

            ui.separator();
            let total = scene_entities.iter().count();
            let groups = scene_entities
                .iter()
                .filter(
                    |(_, _, _, _, g): &(
                        Entity,
                        Option<&Name>,
                        Option<&ChildOf>,
                        Option<&Children>,
                        Option<&GroupMarker>,
                    )| g.is_some(),
                )
                .count();
            ui.label(format!("{} entities, {} groups", total, groups));
        });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn draw_entity_row(
    ui: &mut egui::Ui,
    entity: Entity,
    name: Option<&Name>,
    children: Option<&Children>,
    is_group: bool,
    depth: usize,
    selected_entity: Option<Entity>,
    scene_entities: &Query<
        (Entity, Option<&Name>, Option<&ChildOf>, Option<&Children>, Option<&GroupMarker>),
        With<SceneEntity>,
    >,
    commands: &mut Commands,
    selected_query: &Query<Entity, With<Selected>>,
    hierarchy_state: &mut ResMut<HierarchyState>,
) {
    let display_name = name
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| format!("Entity {:?}", entity));

    let is_selected = selected_entity == Some(entity);

    // Get scene entity children
    let scene_children: Vec<_> = children
        .map(|c| {
            c.iter()
                .filter(|child| scene_entities.get(*child).is_ok())
                .collect()
        })
        .unwrap_or_default();

    let has_children = !scene_children.is_empty();
    let is_expanded = hierarchy_state.expanded.contains(&entity);

    ui.horizontal(|ui| {
        // Indent based on depth
        ui.add_space(depth as f32 * 16.0);

        // Expand/collapse button for groups with children
        if is_group && has_children {
            let arrow = if is_expanded { "▼" } else { "▶" };
            if ui.small_button(arrow).clicked() {
                if is_expanded {
                    hierarchy_state.expanded.remove(&entity);
                } else {
                    hierarchy_state.expanded.insert(entity);
                }
            }
        } else {
            // Indent to align with groups that have expand buttons
            ui.add_space(18.0);
        }

        // Entity name
        let response = ui.selectable_label(is_selected, &display_name);

        if response.clicked() {
            // Clear previous selection
            for selected_e in selected_query.iter() {
                commands.entity(selected_e).remove::<Selected>();
            }
            // Select this entity
            commands.entity(entity).insert(Selected);
        }

        // Icon for groups (right-justified)
        if is_group {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label("📁");
            });
        }
    });

    // Draw children if expanded
    if is_group && is_expanded && has_children {
        for child_entity in scene_children {
            if let Ok((e, child_name, _, child_children, child_is_group)) =
                scene_entities.get(child_entity)
            {
                draw_entity_row(
                    ui,
                    e,
                    child_name,
                    child_children,
                    child_is_group.is_some(),
                    depth + 1,
                    selected_entity,
                    scene_entities,
                    commands,
                    selected_query,
                    hierarchy_state,
                );
            }
        }
    }
}
