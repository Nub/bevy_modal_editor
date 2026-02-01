use bevy::ecs::relationship::Relationship;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use std::collections::HashSet;

use crate::scene::{GroupMarker, PrimitiveMarker, PrimitiveShape, SceneEntity, SceneLightMarker};
use crate::selection::Selected;
use crate::ui::theme::{colors, fonts};

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
    /// Filter text for searching
    pub filter: String,
}

/// Draw the scene hierarchy panel
fn draw_hierarchy_panel(
    mut contexts: EguiContexts,
    scene_entities: Query<
        (
            Entity,
            Option<&Name>,
            Option<&ChildOf>,
            Option<&Children>,
            Option<&GroupMarker>,
            Option<&PrimitiveMarker>,
            Option<&SceneLightMarker>,
        ),
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
        .frame(egui::Frame::side_top_panel(&ctx.style()).fill(colors::PANEL_BG))
        .show(ctx, |ui| {
            // Header
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("Scene Tree")
                        .strong()
                        .size(fonts::TITLE_SIZE)
                        .color(colors::TEXT_PRIMARY),
                );
            });

            ui.add_space(4.0);

            // Filter input
            ui.add(
                egui::TextEdit::singleline(&mut hierarchy_state.filter)
                    .desired_width(ui.available_width())
                    .hint_text(egui::RichText::new("Filter...").color(egui::Color32::from_gray(80)))
                    .margin(egui::vec2(8.0, 6.0)),
            );

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);

            egui::ScrollArea::vertical().show(ui, |ui| {
                // Find root entities (no parent or parent is not a SceneEntity)
                let mut root_entities: Vec<_> = scene_entities
                    .iter()
                    .filter(|(_, _, parent, _, _, _, _)| {
                        parent.map_or(true, |p| scene_entities.get(p.get()).is_err())
                    })
                    .collect();

                // Sort alphabetically by name
                root_entities.sort_by(|a, b| {
                    let name_a = a.1.map(|n| n.as_str()).unwrap_or("");
                    let name_b = b.1.map(|n| n.as_str()).unwrap_or("");
                    name_a.to_lowercase().cmp(&name_b.to_lowercase())
                });

                for (entity, name, _, children, is_group, primitive, light) in root_entities {
                    let entity_name = name.map(|n| n.as_str()).unwrap_or("Entity");

                    // Apply filter
                    if !hierarchy_state.filter.is_empty()
                        && !entity_name.to_lowercase().contains(&hierarchy_state.filter.to_lowercase())
                    {
                        continue;
                    }

                    draw_entity_row(
                        ui,
                        entity,
                        name,
                        children,
                        is_group.is_some(),
                        primitive,
                        light.is_some(),
                        0,
                        selected_entity,
                        &scene_entities,
                        &mut commands,
                        &selected,
                        &mut hierarchy_state,
                    );
                }
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);

            // Footer with counts
            let total = scene_entities.iter().count();
            let groups = scene_entities.iter().filter(|(_, _, _, _, g, _, _)| g.is_some()).count();
            ui.label(
                egui::RichText::new(format!("{} entities, {} groups", total, groups))
                    .small()
                    .color(colors::TEXT_MUTED),
            );
        });
    Ok(())
}

/// Get icon for entity based on its type
fn get_entity_icon(is_group: bool, primitive: Option<&PrimitiveMarker>, is_light: bool) -> &'static str {
    if is_group {
        return "📁";
    }
    if is_light {
        return "💡";
    }
    if let Some(prim) = primitive {
        return match prim.shape {
            PrimitiveShape::Cube => "🔲",
            PrimitiveShape::Sphere => "🔵",
            PrimitiveShape::Cylinder => "🔷",
            PrimitiveShape::Capsule => "💊",
            PrimitiveShape::Plane => "⬜",
        };
    }
    "📦"
}

#[allow(clippy::too_many_arguments)]
fn draw_entity_row(
    ui: &mut egui::Ui,
    entity: Entity,
    name: Option<&Name>,
    children: Option<&Children>,
    is_group: bool,
    primitive: Option<&PrimitiveMarker>,
    is_light: bool,
    depth: usize,
    selected_entity: Option<Entity>,
    scene_entities: &Query<
        (
            Entity,
            Option<&Name>,
            Option<&ChildOf>,
            Option<&Children>,
            Option<&GroupMarker>,
            Option<&PrimitiveMarker>,
            Option<&SceneLightMarker>,
        ),
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

    // Get scene entity children and sort alphabetically
    let mut scene_children: Vec<_> = children
        .map(|c| {
            c.iter()
                .filter(|child| scene_entities.get(*child).is_ok())
                .collect()
        })
        .unwrap_or_default();

    scene_children.sort_by(|a, b| {
        let name_a = scene_entities.get(*a).ok().and_then(|e| e.1.map(|n| n.as_str())).unwrap_or("");
        let name_b = scene_entities.get(*b).ok().and_then(|e| e.1.map(|n| n.as_str())).unwrap_or("");
        name_a.to_lowercase().cmp(&name_b.to_lowercase())
    });

    let has_children = !scene_children.is_empty();
    let is_expanded = hierarchy_state.expanded.contains(&entity);

    // Entity icon
    let icon = get_entity_icon(is_group, primitive, is_light);

    // Build display text with icon and name
    let text_color = if is_selected {
        colors::TEXT_PRIMARY
    } else {
        colors::TEXT_SECONDARY
    };

    let header_text = egui::RichText::new(format!("{} {}", icon, display_name)).color(text_color);

    if has_children {
        // Use CollapsingHeader for items with children
        let id = ui.make_persistent_id(entity);

        let header = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            id,
            is_expanded,
        );

        header
            .show_header(ui, |ui| {
                let button = egui::Button::new(header_text)
                    .fill(if is_selected { colors::SELECTION_BG } else { egui::Color32::TRANSPARENT })
                    .stroke(egui::Stroke::NONE);

                if ui.add(button).clicked() {
                    // Clear previous selection
                    for selected_e in selected_query.iter() {
                        commands.entity(selected_e).remove::<Selected>();
                    }
                    // Select this entity
                    commands.entity(entity).insert(Selected);
                }
            })
            .body(|ui| {
                for child_entity in &scene_children {
                    if let Ok((e, child_name, _, child_children, child_is_group, child_prim, child_light)) =
                        scene_entities.get(*child_entity)
                    {
                        draw_entity_row(
                            ui,
                            e,
                            child_name,
                            child_children,
                            child_is_group.is_some(),
                            child_prim,
                            child_light.is_some(),
                            depth + 1,
                            selected_entity,
                            scene_entities,
                            commands,
                            selected_query,
                            hierarchy_state,
                        );
                    }
                }
            });

        // Sync expanded state with our HierarchyState
        let currently_open = egui::collapsing_header::CollapsingState::load_with_default_open(
            ui.ctx(),
            id,
            is_expanded,
        ).is_open();

        if currently_open && !is_expanded {
            hierarchy_state.expanded.insert(entity);
        } else if !currently_open && is_expanded {
            hierarchy_state.expanded.remove(&entity);
        }
    } else {
        // Simple button for leaf nodes
        ui.horizontal(|ui| {
            // Add indent to align with collapsing headers
            ui.add_space(18.0);

            let button = egui::Button::new(header_text)
                .fill(if is_selected { colors::SELECTION_BG } else { egui::Color32::TRANSPARENT })
                .stroke(egui::Stroke::NONE);

            if ui.add(button).clicked() {
                // Clear previous selection
                for selected_e in selected_query.iter() {
                    commands.entity(selected_e).remove::<Selected>();
                }
                // Select this entity
                commands.entity(entity).insert(Selected);
            }
        });
    }
}
