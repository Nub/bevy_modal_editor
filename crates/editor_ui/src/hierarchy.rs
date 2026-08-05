//! Hierarchy panel (M3-C2): the scene as a tree of `SceneId` entities, rendered
//! into the panel body the dock shell owns. Keyboard-first per the keymap doc's
//! hierarchy context (j/k, gg/G, h/l folds, Enter select, >/< reparent); selection
//! syncs BOTH ways with the viewport; reparent flows through `EditScope` and is
//! undoable like every other mutation (no side door).

use bevy::prelude::*;
use bevy::ui::px;
use editor_core::prelude::*;
use std::collections::{HashMap, HashSet};

use crate::dock::PanelBody;
use crate::style::{self, UiFonts};

pub(crate) const HIERARCHY_PANEL: &str = "hierarchy";

/// Fixed row height (logical px) — the virtualization contract: only the rows
/// inside the scroll viewport (plus overscan) exist as UI nodes, so a 10k-entity
/// scene renders ~30 rows, not 10k (C4).
pub(crate) const ROW_HEIGHT: f32 = 25.0;
const OVERSCAN: usize = 4;

/// Which slice of `rows` to materialize for a viewport.
pub(crate) fn visible_window(scroll_y: f32, view_height: f32, total: usize) -> (usize, usize) {
    if total == 0 {
        return (0, 0);
    }
    let first = ((scroll_y / ROW_HEIGHT).floor() as usize).saturating_sub(OVERSCAN);
    let count = (view_height / ROW_HEIGHT).ceil() as usize + 2 * OVERSCAN;
    let first = first.min(total.saturating_sub(1));
    (first, (first + count).min(total))
}

#[derive(Resource, Default)]
pub(crate) struct HierarchyState {
    pub cursor: usize,
    pub collapsed: HashSet<SceneId>,
    /// Rebuild the row widgets this frame (set by anything that changes the tree).
    dirty: bool,
    /// The materialized window (virtualization) from the last rebuild.
    window: (usize, usize),
    /// Cursor position at the last scroll-follow, to adjust only on movement.
    last_followed: Option<usize>,
}

/// One visible row of the flattened tree (respecting folds).
#[derive(Clone)]
pub(crate) struct Row {
    pub id: SceneId,
    pub parent: Option<SceneId>,
    pub depth: usize,
    pub has_children: bool,
    pub label: String,
}

#[derive(Component)]
pub(crate) struct HierarchyRow(
    #[allow(dead_code, reason = "row identity for future drag/rename")] usize,
);

/// The cursor row (scroll-follow target).
#[derive(Component)]
pub(crate) struct CursorRow;

/// Flatten the scene into visible rows: roots and children sorted by `SceneId`
/// (the same deterministic order the scene format uses), folds respected.
pub(crate) fn build_rows(
    entities: &Query<(Entity, &SceneId, Option<&ChildOf>, Option<&Name>)>,
    scene_ids: &Query<&SceneId>,
    collapsed: &HashSet<SceneId>,
) -> Vec<Row> {
    let mut label_of: HashMap<SceneId, String> = HashMap::new();
    let mut children: HashMap<Option<SceneId>, Vec<SceneId>> = HashMap::new();
    for (_, id, child_of, name) in entities.iter() {
        let parent = child_of
            .and_then(|c| scene_ids.get(c.parent()).ok())
            .copied();
        let label = name
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| format!("entity {}", &id.0.to_string()[..8]));
        label_of.insert(*id, label);
        children.entry(parent).or_default().push(*id);
    }
    for list in children.values_mut() {
        list.sort_by_key(|id| id.0);
    }

    let mut rows = Vec::new();
    fn walk(
        parent: Option<SceneId>,
        depth: usize,
        children: &HashMap<Option<SceneId>, Vec<SceneId>>,
        label_of: &HashMap<SceneId, String>,
        collapsed: &HashSet<SceneId>,
        rows: &mut Vec<Row>,
    ) {
        let Some(list) = children.get(&parent) else {
            return;
        };
        for id in list {
            let has_children = children.contains_key(&Some(*id));
            rows.push(Row {
                id: *id,
                parent,
                depth,
                has_children,
                label: label_of.get(id).cloned().unwrap_or_default(),
            });
            if has_children && !collapsed.contains(id) {
                walk(Some(*id), depth + 1, children, label_of, collapsed, rows);
            }
        }
    }
    walk(None, 0, &children, &label_of, collapsed, &mut rows);
    rows
}

/// The local `Transform` that keeps `target`'s WORLD pose unchanged under its new
/// parent — reparenting must never make an entity visually jump. Rides in the same
/// transaction as the `Reparent` op (one undo entry restores both).
fn world_preserving_local(
    index: &SceneIndex,
    globals: &Query<&GlobalTransform>,
    target: SceneId,
    new_parent: Option<SceneId>,
) -> Option<Transform> {
    let child_global = *globals.get(index.get(&target)?).ok()?;
    let parent_affine = match new_parent {
        Some(parent) => globals.get(index.get(&parent)?).ok()?.affine().inverse(),
        None => return Some(child_global.compute_transform()),
    };
    Some(Transform::from_matrix(
        (parent_affine * child_global.affine()).into(),
    ))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_hierarchy_actions(
    mut reader: MessageReader<ActionInvoked>,
    entities: Query<(Entity, &SceneId, Option<&ChildOf>, Option<&Name>)>,
    scene_ids: Query<&SceneId>,
    index: Res<SceneIndex>,
    globals: Query<&GlobalTransform>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    mut state: ResMut<HierarchyState>,
    mut edits: EditScope,
    mut commands: Commands,
    mut changed: MessageWriter<SelectionChanged>,
) {
    let mut acted = false;
    for invoked in reader.read() {
        let action = invoked.action.as_str();
        if !action.starts_with("hierarchy.") {
            continue;
        }
        acted = true;
        let rows = build_rows(&entities, &scene_ids, &state.collapsed);
        if rows.is_empty() {
            state.cursor = 0;
            continue;
        }
        let cursor = state.cursor.min(rows.len() - 1);
        let row = rows[cursor].clone();
        match action {
            "hierarchy.down" => state.cursor = (cursor + 1).min(rows.len() - 1),
            "hierarchy.up" => state.cursor = cursor.saturating_sub(1),
            "hierarchy.top" => state.cursor = 0,
            "hierarchy.bottom" => state.cursor = rows.len() - 1,
            "hierarchy.select" => {
                let extend = keys
                    .as_ref()
                    .map(|k| k.pressed(KeyCode::ShiftLeft) || k.pressed(KeyCode::ShiftRight))
                    .unwrap_or(false);
                if let Some(entity) = index.get(&row.id) {
                    commands.queue(move |world: &mut World| {
                        // Double-Enter on an instance row = fractal descend
                        // (keymap doc): already selected + is an instance → open.
                        if !extend
                            && world.get::<Selected>(entity).is_some()
                            && world
                                .get::<editor_prefabs::PrefabInstance>(entity)
                                .is_some()
                        {
                            world.write_message(ActionInvoked {
                                action: ActionId::new_static("prefab.open"),
                                args: None,
                                source: InvocationSource::Palette,
                            });
                            return;
                        }
                        // Scoped (open instance): rows outside the scope are inert.
                        let in_scope = world
                            .resource::<SelectionScope>()
                            .0
                            .as_ref()
                            .is_none_or(|scope| scope.contains(&entity));
                        if !in_scope {
                            return;
                        }
                        if !extend {
                            let selected: Vec<Entity> = world
                                .query_filtered::<Entity, With<Selected>>()
                                .iter(world)
                                .collect();
                            for entity in selected {
                                world.entity_mut(entity).remove::<Selected>();
                            }
                        }
                        world.entity_mut(entity).insert(Selected);
                    });
                    changed.write(SelectionChanged);
                }
            }
            "hierarchy.fold" => {
                // h: fold an expanded branch; on a leaf (or folded row), jump to
                // the parent row (tree-plugin convention).
                if row.has_children && !state.collapsed.contains(&row.id) {
                    state.collapsed.insert(row.id);
                } else if let Some(parent) = row.parent
                    && let Some(index) = rows.iter().position(|r| r.id == parent)
                {
                    state.cursor = index;
                }
            }
            "hierarchy.unfold" => {
                state.collapsed.remove(&row.id);
            }
            "hierarchy.reparent-in" => {
                // '>': indent under the previous visible sibling (same parent).
                let sibling = rows[..cursor]
                    .iter()
                    .rev()
                    .find(|r| r.parent == row.parent && r.id != row.id);
                if let Some(sibling) = sibling {
                    let target = sibling.id;
                    state.collapsed.remove(&target);
                    let mut tx = edits.transaction("Reparent").reparent(row.id, Some(target));
                    if let Some(local) =
                        world_preserving_local(&index, &globals, row.id, Some(target))
                    {
                        tx = tx.set(row.id, local);
                    }
                    tx.commit();
                }
            }
            "hierarchy.reparent-out" => {
                // '<': outdent to the grandparent (root if the parent is a root).
                if let Some(parent) = row.parent {
                    let grandparent = rows.iter().find(|r| r.id == parent).and_then(|r| r.parent);
                    let mut tx = edits.transaction("Reparent").reparent(row.id, grandparent);
                    if let Some(local) =
                        world_preserving_local(&index, &globals, row.id, grandparent)
                    {
                        tx = tx.set(row.id, local);
                    }
                    tx.commit();
                }
            }
            _ => {}
        }
    }
    if acted {
        state.dirty = true;
    }
}

/// Anything that changes the tree or its highlights marks the panel dirty:
/// scene edits, selection changes (two-way sync — the cursor FOLLOWS viewport
/// selection), focus/editor transitions.
pub(crate) fn watch_hierarchy_inputs(
    mut edited: MessageReader<Edited>,
    mut selection: MessageReader<SelectionChanged>,
    entities: Query<(Entity, &SceneId, Option<&ChildOf>, Option<&Name>)>,
    scene_ids: Query<&SceneId>,
    selected: Query<&SceneId, With<Selected>>,
    focus: Res<PanelFocus>,
    state_res: Res<EditorState>,
    mut state: ResMut<HierarchyState>,
) {
    let selection_changed = selection.read().next().is_some();
    if edited.read().next().is_some()
        || selection_changed
        || focus.is_changed()
        || state_res.is_changed()
    {
        state.dirty = true;
    }
    // Viewport -> hierarchy sync: cursor jumps to the (first) selected row.
    if selection_changed && let Some(first) = selected.iter().next() {
        let rows = build_rows(&entities, &scene_ids, &state.collapsed);
        if let Some(index) = rows.iter().position(|r| r.id == *first) {
            state.cursor = index;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild_hierarchy(
    mut state: ResMut<HierarchyState>,
    entities: Query<(Entity, &SceneId, Option<&ChildOf>, Option<&Name>)>,
    scene_ids: Query<&SceneId>,
    selected: Query<&SceneId, With<Selected>>,
    instances: Query<&SceneId, With<editor_prefabs::PrefabInstance>>,
    stamped: Query<&SceneId, With<editor_scene::PrefabStamped>>,
    focus: Res<PanelFocus>,
    body: Query<(Entity, &PanelBody)>,
    fonts: Res<UiFonts>,
    settings: Res<EditorSettings>,
    mut commands: Commands,
) {
    if !state.dirty {
        return;
    }
    state.dirty = false;
    let Some((body_entity, _)) = body.iter().find(|(_, b)| b.0.as_str() == HIERARCHY_PANEL) else {
        return;
    };
    let ui = settings.ui.clone();
    let rows = build_rows(&entities, &scene_ids, &state.collapsed);
    state.cursor = state.cursor.min(rows.len().saturating_sub(1));
    let cursor = state.cursor;
    let panel_focused = focus
        .0
        .as_ref()
        .is_some_and(|id| id.as_str() == HIERARCHY_PANEL);
    let selected_ids: HashSet<SceneId> = selected.iter().copied().collect();
    let instance_roots: HashSet<SceneId> = instances.iter().copied().collect();
    let stamped_ids: HashSet<SceneId> = stamped.iter().copied().collect();

    commands.entity(body_entity).despawn_related::<Children>();
    let (first, last) = state.window;
    let last = last.min(rows.len());
    let first = first.min(last);
    commands.entity(body_entity).with_children(|body_children| {
        if rows.is_empty() {
            body_children.spawn((
                Text::new("no entities — i to insert"),
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
            ));
            return;
        }
        // ONE gapless list container: the panel body's row_gap must never leak
        // into the fixed-row-height arithmetic the virtualization depends on.
        let mut list = body_children.spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            ..default()
        });
        list.with_children(|body| {
            // Virtualization spacers stand in for the unmaterialized rows.
            if first > 0 {
                body.spawn(Node {
                    height: px(first as f32 * ROW_HEIGHT),
                    flex_shrink: 0.0,
                    ..default()
                });
            }
            for (i, row) in rows.iter().enumerate().take(last).skip(first) {
                let is_cursor = i == cursor;
                let is_selected = selected_ids.contains(&row.id);
                let mut entity = body.spawn((
                    HierarchyRow(i),
                    Node {
                        align_items: AlignItems::Center,
                        column_gap: px(style::space::XS),
                        height: px(ROW_HEIGHT),
                        padding: UiRect {
                            left: px(style::space::S + row.depth as f32 * style::space::M),
                            right: px(style::space::S),
                            ..default()
                        },
                        border_radius: BorderRadius::all(px(style::radius::S)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(if is_cursor && panel_focused {
                        style::color::selection()
                    } else if is_selected {
                        style::color::selection().with_alpha(0.15)
                    } else {
                        Color::NONE
                    }),
                ));
                if is_cursor {
                    entity.insert(CursorRow);
                }
                entity
                    .observe(
                        move |press: On<Pointer<Press>>,
                              rows_q: Query<&HierarchyRow>,
                              mut state: ResMut<HierarchyState>,
                              mut actions: MessageWriter<ActionInvoked>| {
                            if rows_q.get(press.entity).is_ok() {
                                state.cursor = i;
                                actions.write(ActionInvoked {
                                    action: ActionId::new_static("hierarchy.select"),
                                    args: None,
                                    source: InvocationSource::Palette,
                                });
                            }
                        },
                    )
                    .with_children(|row_node| {
                        // Fold affordance: chevron folded/expanded, · leaf —
                        // nerd-font codepoints (BMP triangles are tofu here).
                        let glyph = if !row.has_children {
                            "·"
                        } else if state.collapsed.contains(&row.id) {
                            style::CHEVRON_RIGHT
                        } else {
                            style::CHEVRON_DOWN
                        };
                        row_node.spawn((
                            Text::new(glyph),
                            style::mono(&fonts, ui.font_size_xs),
                            TextColor(style::color::TEXT_DIM),
                        ));
                        // Prefabs read as GROUPS: ◆ accent on instance roots,
                        // stamped members nested + dimmed but fully live.
                        let is_instance = instance_roots.contains(&row.id);
                        let is_stamped = stamped_ids.contains(&row.id);
                        if is_instance {
                            row_node.spawn((
                                Text::new("◆"),
                                style::mono(&fonts, ui.font_size_xs),
                                TextColor(style::color::accent()),
                            ));
                        }
                        row_node.spawn((
                            Text::new(row.label.clone()),
                            style::sans(&fonts, ui.font_size_s),
                            TextColor(if is_instance || is_selected {
                                style::color::accent()
                            } else if is_stamped {
                                style::color::TEXT_DIM
                            } else {
                                style::color::TEXT_KEYS
                            }),
                        ));
                        // Identity column (owner): short SceneId right-aligned —
                        // a clean second column instead of ragged inline ids.
                        row_node.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        row_node.spawn((
                            Text::new(row.id.0.to_string()[..8].to_string()),
                            style::mono(&fonts, ui.font_size_xs),
                            TextColor(style::color::TEXT_DIM.with_alpha(0.55)),
                        ));
                    });
            }
            if last < rows.len() {
                body.spawn(Node {
                    height: px((rows.len() - last) as f32 * ROW_HEIGHT),
                    flex_shrink: 0.0,
                    ..default()
                });
            }
        });
    });
}

/// Virtualization driver: when scrolling (or a resize) moves the viewport onto
/// rows that aren't materialized, mark the panel dirty so the window re-renders.
pub(crate) fn watch_hierarchy_window(
    entities: Query<(Entity, &SceneId, Option<&ChildOf>, Option<&Name>)>,
    scene_ids: Query<&SceneId>,
    body: Query<(&ComputedNode, &ScrollPosition, &PanelBody)>,
    mut state: ResMut<HierarchyState>,
) {
    for (node, scroll, panel) in &body {
        if panel.0.as_str() != HIERARCHY_PANEL || node.size() == Vec2::ZERO {
            continue;
        }
        let view_height = node.size().y * node.inverse_scale_factor();
        let total = build_rows(&entities, &scene_ids, &state.collapsed).len();
        let window = visible_window(scroll.0.y, view_height, total);
        if window != state.window {
            state.window = window;
            state.dirty = true;
        }
    }
}

/// Keyboard-follow, arithmetic edition (virtualization makes row geometry
/// unreliable — the cursor row may not even be materialized): the fixed row
/// height gives the exact scroll that keeps the cursor visible. Adjusts only
/// when the cursor MOVED, so free wheel-scrolling is never fought.
pub(crate) fn scroll_cursor_into_view(
    mut state: ResMut<HierarchyState>,
    mut body: Query<(&ComputedNode, &mut ScrollPosition, &PanelBody)>,
) {
    if state.last_followed == Some(state.cursor) {
        return;
    }
    for (node, mut scroll, panel) in &mut body {
        if panel.0.as_str() != HIERARCHY_PANEL || node.size() == Vec2::ZERO {
            continue;
        }
        let view_height = node.size().y * node.inverse_scale_factor();
        let top = state.cursor as f32 * ROW_HEIGHT;
        let bottom = top + ROW_HEIGHT;
        if top < scroll.0.y {
            scroll.0.y = top;
        } else if bottom > scroll.0.y + view_height {
            scroll.0.y = bottom - view_height;
        }
        state.last_followed = Some(state.cursor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::EditorCorePlugin;
    use editor_core::prelude::History;

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component)]
    struct Marker;

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("hier-test", "Hier Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Marker>().context("hierarchy");
            for id in [
                "hierarchy.down",
                "hierarchy.up",
                "hierarchy.select",
                "hierarchy.reparent-in",
                "hierarchy.reparent-out",
            ] {
                reg.action(ActionDef::new(id, "t").context("hierarchy").hidden());
            }
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<bevy::input::ButtonInput<KeyCode>>();
        app.init_resource::<HierarchyState>();
        app.add_systems(
            Update,
            handle_hierarchy_actions.in_set(editor_core::EditorSet::Tools),
        );
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;
        app
    }

    fn invoke(app: &mut App, action: &str) {
        app.world_mut().write_message(ActionInvoked {
            action: ActionId::new(action.to_string()),
            args: None,
            source: InvocationSource::Test,
        });
        app.update();
    }

    fn spawn_marker(app: &mut App, id: SceneId) {
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id,
                    components: vec![Box::new(Marker).into_partial_reflect()],
                }],
            });
        app.update();
    }

    // C2: keyboard nav moves the cursor, Enter selects the row's entity, and '>'
    // reparents through EditScope — one undoable history entry, ChildOf real.
    #[test]
    fn nav_select_reparent_undo() {
        let mut app = test_app();
        let (a, b) = (SceneId::random(), SceneId::random());
        spawn_marker(&mut app, a);
        spawn_marker(&mut app, b);

        // Rows sort by uuid: find which row is which.
        let world = app.world_mut();
        let mut entities = world.query::<(Entity, &SceneId, Option<&ChildOf>, Option<&Name>)>();
        let first = {
            let mut ids: Vec<SceneId> = entities.iter(world).map(|(_, id, _, _)| *id).collect();
            ids.sort_by_key(|id| id.0);
            ids[0]
        };
        let second = if first == a { b } else { a };

        // Cursor starts at row 0; j moves to row 1; Enter selects row 1's entity.
        invoke(&mut app, "hierarchy.down");
        assert_eq!(app.world().resource::<HierarchyState>().cursor, 1);
        invoke(&mut app, "hierarchy.select");
        let world = app.world_mut();
        let selected: Vec<SceneId> = world
            .query_filtered::<&SceneId, With<Selected>>()
            .iter(world)
            .copied()
            .collect();
        assert_eq!(selected, vec![second], "Enter selects the cursor row");

        // '>' indents row 1 under its previous sibling (row 0).
        let depth_before = app.world().resource::<History>().undo_depth();
        invoke(&mut app, "hierarchy.reparent-in");
        let world = app.world_mut();
        let index = world.resource::<SceneIndex>();
        let (parent_entity, child_entity) =
            (index.get(&first).unwrap(), index.get(&second).unwrap());
        assert_eq!(
            world.get::<ChildOf>(child_entity).map(|c| c.parent()),
            Some(parent_entity),
            "reparent landed"
        );
        assert_eq!(
            world.resource::<History>().undo_depth(),
            depth_before + 1,
            "one history entry"
        );

        // Undo restores root parentage.
        app.world_mut().resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        let world = app.world_mut();
        let child_entity = world.resource::<SceneIndex>().get(&second).unwrap();
        assert!(
            world.get::<ChildOf>(child_entity).is_none(),
            "undo restores root"
        );
    }
}

#[cfg(test)]
mod virtualization_tests {
    use super::*;

    // C4: window math — 10k rows materialize only a viewport's worth of nodes.
    #[test]
    fn window_is_viewport_sized() {
        let (first, last) = visible_window(0.0, 400.0, 10_000);
        assert_eq!(first, 0);
        assert!(last <= (400.0 / ROW_HEIGHT).ceil() as usize + 8 + 1);

        let (first, last) = visible_window(5_000.0 * ROW_HEIGHT, 400.0, 10_000);
        assert!(first >= 5_000 - 8 && first <= 5_000);
        assert!(last - first <= (400.0 / ROW_HEIGHT).ceil() as usize + 8 + 1);

        // Tail clamps.
        let (first, last) = visible_window(1e9, 400.0, 10_000);
        assert_eq!(last, 10_000);
        assert!(first < 10_000);

        assert_eq!(visible_window(0.0, 400.0, 0), (0, 0));
    }
}
