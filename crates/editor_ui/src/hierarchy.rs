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

#[derive(Resource, Default)]
pub(crate) struct HierarchyState {
    pub cursor: usize,
    pub collapsed: HashSet<SceneId>,
    /// Rebuild the row widgets this frame (set by anything that changes the tree).
    dirty: bool,
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
pub(crate) struct HierarchyRow(#[allow(dead_code, reason = "row identity for future drag/rename")] usize);

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
        let Some(list) = children.get(&parent) else { return };
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
    walk(None, 0, &children, &label_of, &collapsed, &mut rows);
    rows
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_hierarchy_actions(
    mut reader: MessageReader<ActionInvoked>,
    entities: Query<(Entity, &SceneId, Option<&ChildOf>, Option<&Name>)>,
    scene_ids: Query<&SceneId>,
    index: Res<SceneIndex>,
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
                } else if let Some(parent) = row.parent {
                    if let Some(index) = rows.iter().position(|r| r.id == parent) {
                        state.cursor = index;
                    }
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
                    edits
                        .transaction("Reparent")
                        .reparent(row.id, Some(target))
                        .commit();
                }
            }
            "hierarchy.reparent-out" => {
                // '<': outdent to the grandparent (root if the parent is a root).
                if let Some(parent) = row.parent {
                    let grandparent = rows.iter().find(|r| r.id == parent).and_then(|r| r.parent);
                    edits.transaction("Reparent").reparent(row.id, grandparent).commit();
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
    if selection_changed {
        if let Some(first) = selected.iter().next() {
            let rows = build_rows(&entities, &scene_ids, &state.collapsed);
            if let Some(index) = rows.iter().position(|r| r.id == *first) {
                state.cursor = index;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild_hierarchy(
    mut state: ResMut<HierarchyState>,
    entities: Query<(Entity, &SceneId, Option<&ChildOf>, Option<&Name>)>,
    scene_ids: Query<&SceneId>,
    selected: Query<&SceneId, With<Selected>>,
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
    let Some((body_entity, _)) =
        body.iter().find(|(_, b)| b.0.as_str() == HIERARCHY_PANEL)
    else {
        return;
    };
    let ui = settings.ui.clone();
    let rows = build_rows(&entities, &scene_ids, &state.collapsed);
    state.cursor = state.cursor.min(rows.len().saturating_sub(1));
    let cursor = state.cursor;
    let panel_focused =
        focus.0.as_ref().is_some_and(|id| id.as_str() == HIERARCHY_PANEL);
    let selected_ids: HashSet<SceneId> = selected.iter().copied().collect();

    commands.entity(body_entity).despawn_related::<Children>();
    commands.entity(body_entity).with_children(|body| {
        if rows.is_empty() {
            body.spawn((
                Text::new("no entities — i to insert"),
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
            ));
            return;
        }
        for (i, row) in rows.iter().enumerate() {
            let is_cursor = i == cursor;
            let is_selected = selected_ids.contains(&row.id);
            let mut entity = body.spawn((
                HierarchyRow(i),
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(style::space::XS),
                    padding: UiRect {
                        left: px(style::space::S + row.depth as f32 * style::space::M),
                        right: px(style::space::S),
                        top: px(2.0),
                        bottom: px(2.0),
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
                    // Fold affordance: ▸ folded / ▾ expanded / · leaf.
                    let glyph = if !row.has_children {
                        "·"
                    } else if state.collapsed.contains(&row.id) {
                        "▸"
                    } else {
                        "▾"
                    };
                    row_node.spawn((
                        Text::new(glyph),
                        style::mono(&fonts, ui.font_size_xs),
                        TextColor(style::color::TEXT_DIM),
                    ));
                    row_node.spawn((
                        Text::new(row.label.clone()),
                        style::sans(&fonts, ui.font_size_s),
                        TextColor(if is_selected {
                            style::color::accent()
                        } else {
                            style::color::TEXT_KEYS
                        }),
                    ));
                });
        }
    });
}

/// Keep the cursor row visible in the scrollable body (same zeroed-geometry-safe
/// clamp as the palette scroll-follow).
pub(crate) fn scroll_cursor_into_view(
    body: Query<(&ComputedNode, &UiGlobalTransform, &mut ScrollPosition, &PanelBody)>,
    row: Option<Single<(&ComputedNode, &UiGlobalTransform), With<CursorRow>>>,
) {
    let Some(row) = row else { return };
    let (row_node, row_tf) = *row;
    if row_node.size() == Vec2::ZERO {
        return;
    }
    for (cont_node, cont_tf, mut scroll, panel) in body {
        if panel.0.as_str() != HIERARCHY_PANEL {
            continue;
        }
        let scale = cont_node.inverse_scale_factor();
        let cont_h = cont_node.size().y * scale;
        let row_h = row_node.size().y * scale;
        let visible_top = ((row_tf.translation.y - row_node.size().y / 2.0)
            - (cont_tf.translation.y - cont_node.size().y / 2.0))
            * scale;
        let top = visible_top + scroll.0.y;
        let max_scroll = ((cont_node.content_size.y - cont_node.size().y) * scale).max(0.0);
        if top < scroll.0.y {
            scroll.0.y = top.clamp(0.0, max_scroll);
        } else if top + row_h > scroll.0.y + cont_h {
            scroll.0.y = (top + row_h - cont_h).clamp(0.0, max_scroll);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_core::prelude::History;
    use editor_core::EditorCorePlugin;

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
        app.world_mut().resource_mut::<EditQueue>().0.push(Transaction {
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
        assert!(world.get::<ChildOf>(child_entity).is_none(), "undo restores root");
    }
}
