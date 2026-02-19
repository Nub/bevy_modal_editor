use bevy::ecs::relationship::Relationship;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use fuzzy_matcher::skim::SkimMatcherV2;
use fuzzy_matcher::FuzzyMatcher;
use std::collections::HashSet;

use bevy_procedural::ProceduralEntity;

use crate::commands::TakeSnapshotCommand;
use crate::editor::{EditorMode, EditorState, PanelSide, PinnedWindows};
use crate::prefabs::{PrefabEditingContext, PrefabRegistry};
use crate::scene::{GroupMarker, Locked, PrimitiveMarker, PrimitiveShape, SceneEntity, SceneLightMarker};
use crate::selection::Selected;
use crate::ui::theme::{colors, draw_pin_button, panel, panel_frame};

pub struct HierarchyPlugin;

impl Plugin for HierarchyPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<HierarchyState>()
            .add_systems(EguiPrimaryContextPass, draw_hierarchy_panel);
    }
}

/// State for hierarchy panel (expanded nodes, etc.)
#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum HierarchyTab {
    #[default]
    Scene,
    Prefabs,
    World,
}

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
    /// Active tab
    tab: HierarchyTab,
    /// Filter for world tab
    world_filter: String,
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
        (With<SceneEntity>, Without<ProceduralEntity>),
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
        (With<SceneEntity>, Without<ProceduralEntity>),
    >,
    all_entities: Query<(Entity, Option<&Name>, Option<&ChildOf>)>,
    selected: Query<Entity, With<Selected>>,
    mut commands: Commands,
    mut hierarchy_state: ResMut<HierarchyState>,
    mut pinned_window: ResMut<PinnedWindows>,
    prefab_registry: Res<PrefabRegistry>,
    prefab_editing: Option<Res<PrefabEditingContext>>,
    mut spawn_prefab: MessageWriter<crate::prefabs::SpawnPrefabEvent>,
    mut open_prefab: MessageWriter<crate::prefabs::OpenPrefabEvent>,
) -> Result {
    // Don't draw UI when editor is disabled
    if !editor_state.ui_enabled {
        return Ok(());
    }

    // Show hierarchy panel in Hierarchy mode, or when pinned
    let is_pinned = pinned_window.0.contains(&EditorMode::Hierarchy);
    if *current_mode.get() != EditorMode::Hierarchy && !is_pinned {
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

    // Calculate available height using shared panel settings
    let available_height = panel::available_height(ctx);

    // If pinned and the active mode also uses the left side, move to the right
    let displaced = is_pinned
        && *current_mode.get() != EditorMode::Hierarchy
        && current_mode.get().panel_side() == Some(PanelSide::Left);
    let (anchor_align, anchor_offset) = if displaced {
        (egui::Align2::RIGHT_TOP, [-panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    } else {
        (egui::Align2::LEFT_TOP, [panel::WINDOW_PADDING, panel::WINDOW_PADDING])
    };

    egui::Window::new("Scene")
        .default_width(panel::DEFAULT_WIDTH)
        .min_width(panel::MIN_WIDTH)
        .min_height(available_height)
        .max_height(available_height)
        .anchor(anchor_align, anchor_offset)
        .resizable(true)
        .collapsible(false)
        .title_bar(true)
        .scroll(false)
        .frame(panel_frame(&ctx.style()))
        .show(ctx, |ui| {
            // Tab bar with pin button
            ui.horizontal(|ui| {
                if ui.selectable_label(hierarchy_state.tab == HierarchyTab::Scene, "Scene").clicked() {
                    hierarchy_state.tab = HierarchyTab::Scene;
                }
                if ui.selectable_label(hierarchy_state.tab == HierarchyTab::Prefabs, "Prefabs").clicked() {
                    hierarchy_state.tab = HierarchyTab::Prefabs;
                }
                if ui.selectable_label(hierarchy_state.tab == HierarchyTab::World, "World").clicked() {
                    hierarchy_state.tab = HierarchyTab::World;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if draw_pin_button(ui, is_pinned) {
                        if !pinned_window.0.remove(&EditorMode::Hierarchy) {
                            pinned_window.0.insert(EditorMode::Hierarchy);
                        }
                    }
                });
            });
            ui.separator();

            if hierarchy_state.tab == HierarchyTab::World {
                draw_world_browser(ui, &mut hierarchy_state, &all_entities);
                return;
            }

            if hierarchy_state.tab == HierarchyTab::Prefabs {
                draw_prefab_browser(
                    ui,
                    &prefab_registry,
                    &prefab_editing,
                    &mut spawn_prefab,
                    &mut open_prefab,
                );
                return;
            }

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
        commands.queue(TakeSnapshotCommand {
            description: "Reparent entity".to_string(),
        });
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

/// Nerd Font icons (Font Awesome subset)
pub mod icons {
    pub const FOLDER: &str = "\u{f07b}";      //
    pub const FILE: &str = "\u{f15b}";        //
    pub const LIGHTBULB: &str = "\u{f0eb}";   //
    pub const CUBE: &str = "\u{f1b2}";        //
    pub const CIRCLE: &str = "\u{f111}";      //
    pub const DATABASE: &str = "\u{f1c0}";    //  (cylinder-like)
    pub const CAPSULE: &str = "\u{f46b}";     //
    pub const SQUARE: &str = "\u{f0c8}";      //
    pub const BOX: &str = "\u{f466}";         //
    pub const LOCK: &str = "\u{f023}";        //
    pub const RULER: &str = "\u{f546}";       //
    pub const DOT: &str = "\u{f111}";         //  (filled circle)
    pub const SUN: &str = "\u{f185}";         //  (directional light)
}

/// Get icon for entity based on its type
fn get_entity_icon(is_group: bool, primitive: Option<&PrimitiveMarker>, is_light: bool) -> &'static str {
    if is_group {
        return icons::FOLDER;
    }
    if is_light {
        return icons::LIGHTBULB;
    }
    if let Some(prim) = primitive {
        return match prim.shape {
            PrimitiveShape::Cube => icons::CUBE,
            PrimitiveShape::Sphere => icons::CIRCLE,
            PrimitiveShape::Cylinder => icons::DATABASE,
            PrimitiveShape::Capsule => icons::CAPSULE,
            PrimitiveShape::Plane => icons::SQUARE,
        };
    }
    icons::BOX
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
        (With<SceneEntity>, Without<ProceduralEntity>),
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
    let lock_icon = if is_locked { format!("{} ", icons::LOCK) } else { String::new() };

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

/// Draw the prefab browser — lists discovered prefabs with spawn/edit actions.
fn draw_prefab_browser(
    ui: &mut egui::Ui,
    registry: &Res<PrefabRegistry>,
    editing: &Option<Res<PrefabEditingContext>>,
    spawn_events: &mut MessageWriter<crate::prefabs::SpawnPrefabEvent>,
    open_events: &mut MessageWriter<crate::prefabs::OpenPrefabEvent>,
) {
    if let Some(ctx) = editing {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(format!("Editing: {}", ctx.prefab_name))
                    .strong()
                    .color(colors::ACCENT_ORANGE),
            );
        });
        ui.add_space(4.0);
    }

    let mut names: Vec<&str> = registry.names();
    names.sort();

    if names.is_empty() {
        ui.add_space(16.0);
        ui.label(
            egui::RichText::new("No prefabs found")
                .color(colors::TEXT_MUTED)
                .italics(),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("Use \"Create Prefab from Selection\"\nin the command palette (C)")
                .small()
                .color(colors::TEXT_MUTED),
        );
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for name in &names {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(icons::FOLDER)
                        .color(colors::ACCENT_PURPLE),
                );

                ui.label(
                    egui::RichText::new(*name).color(colors::TEXT_PRIMARY),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button(
                            egui::RichText::new("Edit").color(colors::TEXT_SECONDARY),
                        )
                        .on_hover_text("Open prefab for editing")
                        .clicked()
                    {
                        open_events.write(crate::prefabs::OpenPrefabEvent {
                            prefab_name: name.to_string(),
                        });
                    }
                    if ui
                        .small_button(
                            egui::RichText::new("+").strong().color(colors::ACCENT_GREEN),
                        )
                        .on_hover_text("Spawn into scene")
                        .clicked()
                    {
                        spawn_events.write(crate::prefabs::SpawnPrefabEvent {
                            prefab_name: name.to_string(),
                            position: Vec3::ZERO,
                            rotation: Quat::IDENTITY,
                        });
                    }
                });
            });
        }
    });

    ui.add_space(4.0);
    ui.separator();
    ui.label(
        egui::RichText::new(format!("{} prefabs", names.len()))
            .small()
            .color(colors::TEXT_MUTED),
    );
}

/// Draw the world entity browser — lists ALL entities, not just scene entities.
fn draw_world_browser(
    ui: &mut egui::Ui,
    hierarchy_state: &mut ResMut<HierarchyState>,
    all_entities: &Query<(Entity, Option<&Name>, Option<&ChildOf>)>,
) {
    // Filter
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new("Filter").color(colors::TEXT_MUTED));
        ui.text_edit_singleline(&mut hierarchy_state.world_filter);
    });
    ui.add_space(4.0);

    let filter = hierarchy_state.world_filter.to_lowercase();

    // Collect root entities (no parent)
    let mut roots: Vec<(Entity, Option<&Name>)> = all_entities
        .iter()
        .filter(|(_, _, parent)| parent.is_none())
        .map(|(e, name, _)| (e, name))
        .collect();
    roots.sort_by_key(|(e, _)| *e);

    let total = all_entities.iter().count();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for (entity, name) in &roots {
            draw_world_entity_row(ui, *entity, *name, &filter, all_entities, 0);
        }
    });

    ui.add_space(4.0);
    ui.separator();
    ui.label(
        egui::RichText::new(format!("{} total entities", total))
            .small()
            .color(colors::TEXT_MUTED),
    );
}

fn draw_world_entity_row(
    ui: &mut egui::Ui,
    entity: Entity,
    name: Option<&Name>,
    filter: &str,
    all_entities: &Query<(Entity, Option<&Name>, Option<&ChildOf>)>,
    depth: usize,
) {
    let display = if let Some(n) = name {
        format!("{:?}  {}", entity, n.as_str())
    } else {
        format!("{:?}", entity)
    };

    // Collect children
    let mut children: Vec<(Entity, Option<&Name>)> = all_entities
        .iter()
        .filter(|(_, _, parent)| parent.map(|p| p.get()) == Some(entity))
        .map(|(e, n, _)| (e, n))
        .collect();
    children.sort_by_key(|(e, _)| *e);

    // Filter: skip if neither this entity nor any descendant matches
    let matches_filter = filter.is_empty() || display.to_lowercase().contains(filter);
    if !matches_filter && children.is_empty() {
        return;
    }

    if children.is_empty() {
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 16.0 + 18.0);
            ui.label(egui::RichText::new(&display).color(colors::TEXT_SECONDARY));
        });
    } else {
        let id = ui.make_persistent_id(("world", entity));
        egui::CollapsingHeader::new(
            egui::RichText::new(&display).color(colors::TEXT_SECONDARY),
        )
        .id_salt(id)
        .default_open(depth == 0)
        .show(ui, |ui| {
            for (child_entity, child_name) in &children {
                draw_world_entity_row(ui, *child_entity, *child_name, filter, all_entities, depth + 1);
            }
        });
    }
}
