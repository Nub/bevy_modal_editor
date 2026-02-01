use bevy::ecs::relationship::Relationship;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::collections::HashSet;

use crate::editor::{EditorMode, EditorState};
use crate::scene::{GroupMarker, Locked, PrimitiveMarker, PrimitiveShape, SceneEntity, SceneLightMarker};
use crate::selection::Selected;
use crate::ui::theme::colors;

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
    /// Filter query for fuzzy search
    pub filter: String,
    /// Whether the filter input should be focused
    pub focus_filter: bool,
    /// Whether the filter is active (visible even when empty)
    pub filter_active: bool,
}

/// Payload for drag and drop operations
#[derive(Clone, Copy)]
struct DragPayload(Entity);

/// Compute the set of entities that should be visible based on the fuzzy filter.
/// Includes matching entities and all their ancestors (to maintain hierarchy structure).
fn compute_visible_entities(
    filter: &str,
    scene_entities: &Query<
        (
            Entity,
            Option<&Name>,
            Option<&ChildOf>,
            Option<&Children>,
            Option<&GroupMarker>,
            Option<&PrimitiveMarker>,
            Option<&SceneLightMarker>,
            Option<&Locked>,
        ),
        With<SceneEntity>,
    >,
) -> HashSet<Entity> {
    let matcher = SkimMatcherV2::default();
    let mut visible = HashSet::new();

    // First pass: find all entities that match the filter
    let mut matching_entities = Vec::new();
    for (entity, name, _, _, _, _, _, _) in scene_entities.iter() {
        let display_name = name.map(|n| n.as_str()).unwrap_or("");
        if matcher.fuzzy_match(display_name, filter).is_some() {
            matching_entities.push(entity);
            visible.insert(entity);
        }
    }

    // Second pass: add all ancestors of matching entities
    for entity in matching_entities {
        let mut current = entity;
        while let Ok((_, _, parent, _, _, _, _, _)) = scene_entities.get(current) {
            if let Some(parent_ref) = parent {
                let parent_entity = parent_ref.get();
                if scene_entities.get(parent_entity).is_ok() {
                    visible.insert(parent_entity);
                    current = parent_entity;
                } else {
                    break;
                }
            } else {
                break;
            }
        }
    }

    visible
}

/// Draw the scene hierarchy panel
fn draw_hierarchy_panel(
    mut contexts: EguiContexts,
    current_mode: Res<State<EditorMode>>,
    keyboard: Res<ButtonInput<KeyCode>>,
    editor_state: Res<EditorState>,
    scene_entities: Query<
        (
            Entity,
            Option<&Name>,
            Option<&ChildOf>,
            Option<&Children>,
            Option<&GroupMarker>,
            Option<&PrimitiveMarker>,
            Option<&SceneLightMarker>,
            Option<&Locked>,
        ),
        With<SceneEntity>,
    >,
    selected: Query<Entity, With<Selected>>,
    mut commands: Commands,
    mut hierarchy_state: ResMut<HierarchyState>,
) -> Result {
    // Don't draw UI when editor is disabled
    if !editor_state.ui_enabled {
        return Ok(());
    }

    // Only show hierarchy panel in Hierarchy mode
    if *current_mode.get() != EditorMode::Hierarchy {
        // Clear filter when leaving hierarchy mode
        hierarchy_state.filter.clear();
        hierarchy_state.filter_active = false;
        return Ok(());
    }

    let ctx = contexts.ctx_mut()?;

    // Handle F key to start filtering (only if UI doesn't want keyboard input)
    // "/" opens the FindObject palette instead
    if !ctx.wants_keyboard_input() && keyboard.just_pressed(KeyCode::KeyF) {
        hierarchy_state.focus_filter = true;
        hierarchy_state.filter_active = true;
    }

    let selected_entities: HashSet<Entity> = selected.iter().collect();
    let shift_held = ctx.input(|i| i.modifiers.shift);

    // Build set of visible entities based on filter
    let visible_entities = if hierarchy_state.filter.is_empty() {
        None // No filter, show all
    } else {
        Some(compute_visible_entities(&hierarchy_state.filter, &scene_entities))
    };

    // Track reparenting operation to apply after UI
    let mut reparent_op: Option<(Entity, Option<Entity>)> = None;

    // Floating window padding from edges
    let window_padding = 8.0;
    let status_bar_height = 24.0;
    let available_height = ctx.content_rect().height() - status_bar_height - window_padding * 2.0;

    egui::Window::new("Scene")
        .default_size([250.0, available_height])
        .min_width(250.0)
        .min_height(100.0)
        .max_height(available_height)
        .anchor(egui::Align2::LEFT_TOP, [window_padding, window_padding])
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(false)
        .frame(
            egui::Frame::window(&ctx.style())
                .fill(colors::PANEL_BG)
                .shadow(egui::Shadow {
                    offset: [0, 2],
                    blur: 4,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(40),
                }),
        )
        .show(ctx, |ui| {
            // Force the window content to fill available height
            let title_bar_height = 28.0;
            let footer_height = 30.0;
            ui.set_min_height(available_height - title_bar_height - footer_height);

            // Show filter input if filter is active or has content
            let show_filter = hierarchy_state.filter_active || !hierarchy_state.filter.is_empty();
            if show_filter {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("🔍").color(colors::TEXT_MUTED));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut hierarchy_state.filter)
                            .hint_text("Filter...")
                            .desired_width(ui.available_width() - 24.0),
                    );

                    // Focus the filter input when F is pressed
                    if hierarchy_state.focus_filter {
                        response.request_focus();
                        hierarchy_state.focus_filter = false;
                    }

                    // Clear filter and deactivate on Escape
                    if response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        hierarchy_state.filter.clear();
                        hierarchy_state.filter_active = false;
                    }

                    // Deactivate filter when it loses focus and is empty
                    if response.lost_focus() && hierarchy_state.filter.is_empty() {
                        hierarchy_state.filter_active = false;
                    }
                });
                ui.add_space(4.0);
            }

            egui::ScrollArea::vertical().show(ui, |ui| {
                // Find root entities (no parent or parent is not a SceneEntity)
                let mut root_entities: Vec<_> = scene_entities
                    .iter()
                    .filter(|(entity, _, parent, _, _, _, _, _)| {
                        // Must be a root entity
                        let is_root = parent.map_or(true, |p| scene_entities.get(p.get()).is_err());
                        // Must be visible (if filtering)
                        let is_visible = visible_entities.as_ref().map_or(true, |v| v.contains(entity));
                        is_root && is_visible
                    })
                    .collect();

                // Sort alphabetically by name
                root_entities.sort_by(|a, b| {
                    let name_a = a.1.map(|n| n.as_str()).unwrap_or("");
                    let name_b = b.1.map(|n| n.as_str()).unwrap_or("");
                    name_a.to_lowercase().cmp(&name_b.to_lowercase())
                });

                // Make the whole scroll area a drop zone for unparenting
                let scroll_response = ui.interact(
                    ui.available_rect_before_wrap(),
                    ui.id().with("root_drop"),
                    egui::Sense::hover(),
                );

                for (entity, name, _, children, is_group, primitive, light, locked) in root_entities {
                    if let Some(op) = draw_entity_row(
                        ui,
                        entity,
                        name,
                        children,
                        is_group.is_some(),
                        primitive,
                        light.is_some(),
                        locked.is_some(),
                        0,
                        &selected_entities,
                        shift_held,
                        &scene_entities,
                        &mut commands,
                        &selected,
                        &mut hierarchy_state,
                        &visible_entities,
                    ) {
                        reparent_op = Some(op);
                    }
                }

                // Check for drops at root level (unparent)
                if scroll_response.hovered() && ui.input(|i| i.pointer.any_released()) {
                    if let Some(payload) = ui.ctx().memory(|mem| mem.data.get_temp::<DragPayload>(egui::Id::NULL)) {
                        // Only unparent if not already at root
                        if scene_entities.get(payload.0).ok().and_then(|e| e.2).is_some() {
                            reparent_op = Some((payload.0, None));
                        }
                    }
                }
            });

            ui.add_space(4.0);
            ui.separator();
            ui.add_space(2.0);

            // Footer with counts
            let total = scene_entities.iter().count();
            let groups = scene_entities.iter().filter(|(_, _, _, _, g, _, _, _)| g.is_some()).count();
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

    // Clear drag payload if pointer was released
    if ctx.input(|i| i.pointer.any_released()) {
        ctx.memory_mut(|mem| {
            mem.data.remove::<DragPayload>(egui::Id::NULL);
        });
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
    is_locked: bool,
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
            Option<&Locked>,
        ),
        With<SceneEntity>,
    >,
    commands: &mut Commands,
    selected_query: &Query<Entity, With<Selected>>,
    hierarchy_state: &mut ResMut<HierarchyState>,
    visible_entities: &Option<HashSet<Entity>>,
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
                .filter(|child| {
                    // Must be a scene entity
                    let is_scene_entity = scene_entities.get(*child).is_ok();
                    // Must be visible (if filtering)
                    let is_visible = visible_entities.as_ref().map_or(true, |v| v.contains(child));
                    is_scene_entity && is_visible
                })
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
    let lock_icon = if is_locked { "🔒 " } else { "" };

    // Build display text with icon and name
    let text_color = if is_locked {
        colors::TEXT_MUTED
    } else if is_selected {
        colors::TEXT_PRIMARY
    } else {
        colors::TEXT_SECONDARY
    };

    let header_text = egui::RichText::new(format!("{}{} {}", lock_icon, icon, display_name)).color(text_color);
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
                let response = draw_draggable_button(
                    ui,
                    entity,
                    drag_id,
                    header_text.clone(),
                    is_selected,
                    shift_held,
                    is_group,
                    is_locked,
                    &scene_children,
                    commands,
                    selected_query,
                );

                // Check if something was dropped on this group
                if is_group && response.hovered() && ui.input(|i| i.pointer.any_released()) {
                    if let Some(payload) = ui.ctx().memory(|mem| mem.data.get_temp::<DragPayload>(egui::Id::NULL)) {
                        let dragged_entity = payload.0;
                        // Don't parent to self or own children
                        if dragged_entity != entity {
                            reparent_op = Some((dragged_entity, Some(entity)));
                        }
                    }
                }

                // Visual feedback when dragging over a group
                let is_dragging = ui.ctx().memory(|mem| mem.data.get_temp::<DragPayload>(egui::Id::NULL).is_some());
                if is_group && is_dragging && response.hovered() {
                    ui.painter().rect_stroke(
                        response.rect,
                        2.0,
                        egui::Stroke::new(2.0, colors::ACCENT_BLUE),
                        egui::StrokeKind::Inside,
                    );
                }
            })
            .body(|ui| {
                for child_entity in &scene_children {
                    if let Ok((e, child_name, _, child_children, child_is_group, child_prim, child_light, child_locked)) =
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
                            child_locked.is_some(),
                            depth + 1,
                            selected_entities,
                            shift_held,
                            scene_entities,
                            commands,
                            selected_query,
                            hierarchy_state,
                            visible_entities,
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
                false,
                is_locked,
                &[],
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
    is_group: bool,
    is_locked: bool,
    children: &[Entity],
    commands: &mut Commands,
    selected_query: &Query<Entity, With<Selected>>,
) -> egui::Response {
    let button = egui::Button::new(text.clone())
        .fill(if is_selected { colors::SELECTION_BG } else { egui::Color32::TRANSPARENT })
        .stroke(egui::Stroke::NONE)
        .sense(egui::Sense::click_and_drag());

    let response = ui.add(button);

    // Handle right-click on groups to select all children
    if response.secondary_clicked() && is_group && !children.is_empty() {
        // Clear previous selection
        for selected_e in selected_query.iter() {
            commands.entity(selected_e).remove::<Selected>();
        }
        // Select all children
        for &child in children {
            commands.entity(child).insert(Selected);
        }
    }
    // Handle left-click for selection (only if not dragging)
    else if response.clicked() {
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

    // Handle drag (locked items cannot be dragged)
    if response.drag_started() && !is_locked {
        ui.ctx().memory_mut(|mem| {
            mem.data.insert_temp(egui::Id::NULL, DragPayload(entity));
        });
    }

    if response.dragged() {
        // Show drag preview at cursor
        if let Some(pos) = ui.ctx().pointer_hover_pos() {
            egui::Area::new(drag_id.with("preview"))
                .fixed_pos(pos + egui::vec2(10.0, 10.0))
                .order(egui::Order::Tooltip)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style())
                        .fill(colors::BG_DARK)
                        .show(ui, |ui| {
                            ui.label(text);
                        });
                });
        }
    }

    // Clear drag payload when released (after a small delay to allow drop detection)
    if response.drag_stopped() {
        // The payload is read by drop targets on release, then cleared next frame
    }

    response
}
