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
    /// Entity being dragged (if any)
    pub dragging: Option<Entity>,
}

/// Payload for drag and drop operations
#[derive(Clone, Copy)]
struct DragPayload(Entity);

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
    let selected_entities: HashSet<Entity> = selected.iter().collect();
    let shift_held = ctx.input(|i| i.modifiers.shift);

    // Track reparenting operation to apply after UI
    let mut reparent_op: Option<(Entity, Option<Entity>)> = None;

    egui::SidePanel::left("hierarchy_panel")
        .default_width(200.0)
        .frame(egui::Frame::side_top_panel(&ctx.style()).fill(colors::PANEL_BG))
        .show(ctx, |ui| {
            // Header
            ui.add_space(8.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new("Scene")
                        .strong()
                        .size(fonts::TITLE_SIZE)
                        .color(colors::TEXT_PRIMARY),
                );
            });

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
                    if let Some(op) = draw_entity_row(
                        ui,
                        entity,
                        name,
                        children,
                        is_group.is_some(),
                        primitive,
                        light.is_some(),
                        0,
                        &selected_entities,
                        shift_held,
                        &scene_entities,
                        &mut commands,
                        &selected,
                        &mut hierarchy_state,
                    ) {
                        reparent_op = Some(op);
                    }
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

    // Apply reparenting after UI is done
    if let Some((child, new_parent)) = reparent_op {
        if let Some(parent) = new_parent {
            commands.entity(child).set_parent_in_place(parent);
        } else {
            commands.entity(child).remove_parent_in_place();
        }
    }

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

/// Returns Some((child, new_parent)) if a reparent operation should occur
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
    selected_entities: &HashSet<Entity>,
    shift_held: bool,
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
) -> Option<(Entity, Option<Entity>)> {
    let mut reparent_op = None;

    let display_name = name
        .map(|n| n.as_str().to_string())
        .unwrap_or_else(|| format!("Entity {:?}", entity));

    let is_selected = selected_entities.contains(&entity);

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
    let drag_id = egui::Id::new(("hierarchy_drag", entity));

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
                // Make this a drop target if it's a group
                let response = if is_group {
                    let (inner_response, payload) = ui.dnd_drop_zone::<DragPayload, _>(egui::Frame::NONE, |ui| {
                        draw_draggable_button(
                            ui,
                            entity,
                            drag_id,
                            header_text.clone(),
                            is_selected,
                            shift_held,
                            commands,
                            selected_query,
                        )
                    });

                    // Check if something was dropped on this group
                    if let Some(payload) = payload {
                        let dragged_entity = payload.0;
                        // Don't parent to self
                        if dragged_entity != entity {
                            reparent_op = Some((dragged_entity, Some(entity)));
                        }
                    }

                    inner_response.inner
                } else {
                    draw_draggable_button(
                        ui,
                        entity,
                        drag_id,
                        header_text.clone(),
                        is_selected,
                        shift_held,
                        commands,
                        selected_query,
                    )
                };

                // Visual feedback when dragging over a group
                if is_group && ui.ctx().dragged_id().is_some() {
                    if response.hovered() {
                        ui.painter().rect_stroke(
                            response.rect,
                            2.0,
                            egui::Stroke::new(2.0, colors::ACCENT_BLUE),
                            egui::StrokeKind::Inside,
                        );
                    }
                }
            })
            .body(|ui| {
                for child_entity in &scene_children {
                    if let Ok((e, child_name, _, child_children, child_is_group, child_prim, child_light)) =
                        scene_entities.get(*child_entity)
                    {
                        if let Some(op) = draw_entity_row(
                            ui,
                            e,
                            child_name,
                            child_children,
                            child_is_group.is_some(),
                            child_prim,
                            child_light.is_some(),
                            depth + 1,
                            selected_entities,
                            shift_held,
                            scene_entities,
                            commands,
                            selected_query,
                            hierarchy_state,
                        ) {
                            reparent_op = Some(op);
                        }
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

            draw_draggable_button(
                ui,
                entity,
                drag_id,
                header_text,
                is_selected,
                shift_held,
                commands,
                selected_query,
            );
        });
    }

    reparent_op
}

/// Draw a draggable button for an entity
fn draw_draggable_button(
    ui: &mut egui::Ui,
    entity: Entity,
    drag_id: egui::Id,
    text: egui::RichText,
    is_selected: bool,
    shift_held: bool,
    commands: &mut Commands,
    selected_query: &Query<Entity, With<Selected>>,
) -> egui::Response {
    // Use egui's drag source API
    let response = ui.dnd_drag_source(drag_id, DragPayload(entity), |ui| {
        let button = egui::Button::new(text)
            .fill(if is_selected { colors::SELECTION_BG } else { egui::Color32::TRANSPARENT })
            .stroke(egui::Stroke::NONE);

        ui.add(button)
    }).response;

    // Handle click for selection
    if response.clicked() {
        if shift_held {
            if is_selected {
                commands.entity(entity).remove::<Selected>();
            } else {
                commands.entity(entity).insert(Selected);
            }
        } else {
            for selected_e in selected_query.iter() {
                commands.entity(selected_e).remove::<Selected>();
            }
            commands.entity(entity).insert(Selected);
        }
    }

    response
}
