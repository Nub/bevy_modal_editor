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

pub(crate) use crate::list::{ROW_HEIGHT, visible_window};

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

/// A row being dragged onto another row, and the row it is over.
///
/// Reparenting already existed on `>` and `<`, which indent under the previous
/// sibling and outdent to the grandparent — precise, and no help at all when
/// the new parent is somewhere else entirely. Dragging says where directly.
#[derive(Resource, Default)]
pub(crate) struct HierarchyDrag {
    /// What is being carried.
    pub source: Option<SceneId>,
    /// What it is currently over, for the drop highlight.
    pub over: Option<SceneId>,
    /// A completed drop, waiting for the system that owns `EditScope`.
    pub drop: Option<(SceneId, Option<SceneId>)>,
}

/// Is `target` inside `source`'s own subtree?
///
/// Dropping a group into its own child would make a cycle: the entities become
/// unreachable from any root, the hierarchy panel stops listing them, and scene
/// capture — which walks from roots — writes a file missing them. Refusing is
/// the only sane answer, and it has to be said out loud rather than ignored.
pub(crate) fn is_inside(rows: &[Row], source: SceneId, target: SceneId) -> bool {
    let mut current = Some(target);
    while let Some(id) = current {
        if id == source {
            return true;
        }
        current = rows
            .iter()
            .find(|row| row.id == id)
            .and_then(|row| row.parent);
    }
    false
}

/// Commit a dropped row, through the same path `>` and `<` use.
pub(crate) fn perform_hierarchy_drop(
    mut drag: ResMut<HierarchyDrag>,
    entities: Query<(
        Entity,
        &SceneId,
        Option<&ChildOf>,
        Option<&Name>,
        Has<editor_scene::models::MeshRef>,
        Has<editor_core::lock::Locked>,
    )>,
    scene_ids: Query<&SceneId>,
    hidden: Res<editor_core::hide::Hidden>,
    state: Res<HierarchyState>,
    index: Res<SceneIndex>,
    globals: Query<&GlobalTransform>,
    mut edits: EditScope,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    let Some((source, target)) = drag.drop.take() else {
        return;
    };
    if Some(source) == target {
        return;
    }
    let rows = build_rows(&entities, &scene_ids, &hidden, &state.collapsed);
    if let Some(target) = target
        && is_inside(&rows, source, target)
    {
        feedback.write(editor_scene::SceneIoFeedback {
            message: "a group cannot go inside itself".into(),
            success: false,
        });
        return;
    }
    // The world pose is preserved, exactly as the keyboard verbs do it: a
    // reparent is a change of OWNERSHIP, and an object that jumped across the
    // level because you re-filed it would be a different edit than the one
    // asked for.
    let mut tx = edits.transaction("Reparent").reparent(source, target);
    if let Some(local) = world_preserving_local(&index, &globals, source, target) {
        tx = tx.set(source, local);
    }
    tx.commit();
    let name = rows
        .iter()
        .find(|row| row.id == source)
        .map(|row| row.label.clone())
        .unwrap_or_else(|| "object".into());
    let into = target
        .and_then(|id| rows.iter().find(|row| row.id == id))
        .map(|row| row.label.clone());
    feedback.write(editor_scene::SceneIoFeedback {
        message: match into {
            Some(parent) => format!("{name} \u{2192} {parent}"),
            None => format!("{name} \u{2192} top level"),
        },
        success: true,
    });
}

/// One visible row of the flattened tree (respecting folds).
#[derive(Clone)]
pub(crate) struct Row {
    pub id: SceneId,
    pub parent: Option<SceneId>,
    pub depth: usize,
    pub has_children: bool,
    pub label: String,
    /// A placed model still LINKED to its source: its geometry lives in the
    /// derived gltf subtree, which carries no `SceneId` and so cannot appear
    /// as rows. Without a badge, "linked model" and "empty entity" look
    /// identical here — and whether a model is flattened decides what you can
    /// select, shade, and hang colliders on.
    pub linked_model: bool,
    /// Locked: this object refuses edits. The hierarchy is where you scan a
    /// level, so it is where "why won't this move?" is answered without
    /// clicking anything.
    pub locked: bool,
    /// Hidden: taken out of the view, still in the level. The hierarchy is
    /// the ONLY way back to a hidden object, so its row stays fully clickable
    /// and only its rendering changes.
    pub hidden: bool,
}

#[derive(Component)]
pub(crate) struct HierarchyRow(pub(crate) usize);

/// The cursor row (scroll-follow target).
#[derive(Component)]
pub(crate) struct CursorRow;

/// Flatten the scene into visible rows: roots and children sorted by `SceneId`
/// (the same deterministic order the scene format uses), folds respected.
pub(crate) fn build_rows(
    entities: &Query<(
        Entity,
        &SceneId,
        Option<&ChildOf>,
        Option<&Name>,
        Has<editor_scene::models::MeshRef>,
        Has<editor_core::lock::Locked>,
    )>,
    scene_ids: &Query<&SceneId>,
    hidden: &editor_core::hide::Hidden,
    collapsed: &HashSet<SceneId>,
) -> Vec<Row> {
    let mut label_of: HashMap<SceneId, String> = HashMap::new();
    let mut children: HashMap<Option<SceneId>, Vec<SceneId>> = HashMap::new();
    let mut linked: HashSet<SceneId> = HashSet::new();
    let mut locked: HashSet<SceneId> = HashSet::new();
    for (_, id, child_of, name, linked_model, is_locked) in entities.iter() {
        let parent = child_of
            .and_then(|c| scene_ids.get(c.parent()).ok())
            .copied();
        let label = name
            .map(|n| n.as_str().to_string())
            .unwrap_or_else(|| format!("entity {}", &id.0.to_string()[..8]));
        label_of.insert(*id, label);
        if linked_model {
            linked.insert(*id);
        }
        if is_locked {
            locked.insert(*id);
        }
        children.entry(parent).or_default().push(*id);
    }
    for list in children.values_mut() {
        list.sort_by_key(|id| id.0);
    }
    let mut rows = Vec::new();
    #[allow(clippy::too_many_arguments)]
    fn walk(
        parent: Option<SceneId>,
        parent_hidden: bool,
        depth: usize,
        children: &HashMap<Option<SceneId>, Vec<SceneId>>,
        label_of: &HashMap<SceneId, String>,
        linked: &HashSet<SceneId>,
        locked: &HashSet<SceneId>,
        hidden: &editor_core::hide::Hidden,
        collapsed: &HashSet<SceneId>,
        rows: &mut Vec<Row>,
    ) {
        let Some(list) = children.get(&parent) else {
            return;
        };
        for id in list {
            let has_children = children.contains_key(&Some(*id));
            // Hidden-ness INHERITS down the row tree, the way visibility does in
            // the viewport: the children of a hidden group are not in the hidden
            // set themselves, and would otherwise read as visible.
            let row_hidden = parent_hidden || hidden.contains(*id);
            rows.push(Row {
                id: *id,
                parent,
                depth,
                has_children,
                label: label_of.get(id).cloned().unwrap_or_default(),
                linked_model: linked.contains(id),
                locked: locked.contains(id),
                hidden: row_hidden,
            });
            if has_children && !collapsed.contains(id) {
                walk(
                    Some(*id),
                    row_hidden,
                    depth + 1,
                    children,
                    label_of,
                    linked,
                    locked,
                    hidden,
                    collapsed,
                    rows,
                );
            }
        }
    }
    walk(
        None, false, 0, &children, &label_of, &linked, &locked, hidden, collapsed, &mut rows,
    );
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
    entities: Query<(
        Entity,
        &SceneId,
        Option<&ChildOf>,
        Option<&Name>,
        Has<editor_scene::models::MeshRef>,
        Has<editor_core::lock::Locked>,
    )>,
    scene_ids: Query<&SceneId>,
    hidden: Res<editor_core::hide::Hidden>,
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
        let rows = build_rows(&entities, &scene_ids, &hidden, &state.collapsed);
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
    entities: Query<(
        Entity,
        &SceneId,
        Option<&ChildOf>,
        Option<&Name>,
        Has<editor_scene::models::MeshRef>,
        Has<editor_core::lock::Locked>,
    )>,
    scene_ids: Query<&SceneId>,
    hidden: Res<editor_core::hide::Hidden>,
    selected: Query<&SceneId, With<Selected>>,
    focus: Res<PanelFocus>,
    state_res: Res<EditorState>,
    drag: Res<HierarchyDrag>,
    mut state: ResMut<HierarchyState>,
) {
    let selection_changed = selection.read().next().is_some();
    if edited.read().next().is_some()
        || selection_changed
        || focus.is_changed()
        || state_res.is_changed()
        // Hiding changes what every row looks like, and nothing else fires.
        || hidden.is_changed()
        // The drop highlight lives in the row widgets, so a moving target has
        // to rebuild them.
        || drag.is_changed()
    {
        state.dirty = true;
    }
    // Viewport -> hierarchy sync: cursor jumps to the (first) selected row.
    if selection_changed && let Some(first) = selected.iter().next() {
        let rows = build_rows(&entities, &scene_ids, &hidden, &state.collapsed);
        if let Some(index) = rows.iter().position(|r| r.id == *first) {
            state.cursor = index;
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild_hierarchy(
    mut state: ResMut<HierarchyState>,
    entities: Query<(
        Entity,
        &SceneId,
        Option<&ChildOf>,
        Option<&Name>,
        Has<editor_scene::models::MeshRef>,
        Has<editor_core::lock::Locked>,
    )>,
    scene_ids: Query<&SceneId>,
    hidden: Res<editor_core::hide::Hidden>,
    drag: Res<HierarchyDrag>,
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
    // Dropping on the empty space BELOW the tree means the top level. Without
    // it, the only way to un-parent by mouse would be to find some root to
    // drop onto and then drag out again.
    commands.entity(body_entity).observe(
        move |drop: On<Pointer<DragDrop>>, mut drag: ResMut<HierarchyDrag>| {
            let _ = drop;
            if let Some(source) = drag.source.take() {
                drag.drop = Some((source, None));
            }
            drag.over = None;
        },
    );
    let ui = settings.ui.clone();
    let rows = build_rows(&entities, &scene_ids, &hidden, &state.collapsed);
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
                    BackgroundColor(if drag.over == Some(row.id) {
                        // Where it would LAND. A drag with no target shown is
                        // a guess with a commit at the end of it.
                        style::color::accent().with_alpha(0.28)
                    } else if is_cursor && panel_focused {
                        style::color::selection()
                    } else if is_selected {
                        // Authored for LINEAR blending: UI alpha composites in
                        // linear space, so tiny sRGB alphas read far stronger
                        // over these darks — 0.03 lands as a quiet wash.
                        style::color::selection().with_alpha(0.03)
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
                    // Drag to re-file. `>` and `<` indent under the previous
                    // sibling and outdent to the grandparent — exact, and no
                    // help when the new parent is somewhere else entirely.
                    .observe({
                        let id = row.id;
                        move |_: On<Pointer<DragStart>>, mut drag: ResMut<HierarchyDrag>| {
                            drag.source = Some(id);
                            drag.over = None;
                        }
                    })
                    .observe({
                        let id = row.id;
                        move |_: On<Pointer<DragOver>>, mut drag: ResMut<HierarchyDrag>| {
                            if drag.source.is_some_and(|source| source != id) {
                                drag.over = Some(id);
                            }
                        }
                    })
                    .observe({
                        let id = row.id;
                        move |_: On<Pointer<DragLeave>>, mut drag: ResMut<HierarchyDrag>| {
                            if drag.over == Some(id) {
                                drag.over = None;
                            }
                        }
                    })
                    .observe({
                        let id = row.id;
                        move |_: On<Pointer<DragDrop>>, mut drag: ResMut<HierarchyDrag>| {
                            // The DROP decides, not the drag: the pointer can
                            // leave and re-enter rows on the way, and only
                            // where it is released is an instruction.
                            if let Some(source) = drag.source.take() {
                                drag.drop = Some((source, Some(id)));
                            }
                            drag.over = None;
                        }
                    })
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
                            TextColor(if row.hidden {
                                // Hidden outranks every other tier: the row is
                                // the only way back to the object, so it has to
                                // read as absent from the viewport at a glance.
                                style::color::TEXT_DIM.with_alpha(0.6)
                            } else if is_instance {
                                style::color::accent()
                            } else if is_selected {
                                style::color::TEXT_BRIGHT
                            } else if is_stamped {
                                style::color::TEXT_DIM
                            } else {
                                style::color::TEXT_KEYS
                            }),
                        ));
                        if row.hidden {
                            row_node.spawn((
                                Text::new(style::glyph::EYE_SLASH),
                                style::mono(&fonts, ui.font_size_xs),
                                TextColor(style::color::TEXT_DIM),
                                Node {
                                    margin: UiRect::left(px(style::space::XS)),
                                    ..default()
                                },
                            ));
                        }
                        // The padlock rides with the NAME rather than in a
                        // far column: it is the answer to "why did nothing
                        // happen when I moved that?", and that question is
                        // asked while looking at the object, not the margin.
                        if row.locked {
                            row_node.spawn((
                                Text::new(style::glyph::LOCK),
                                style::mono(&fonts, ui.font_size_xs),
                                TextColor(style::color::TEXT_WARN),
                                Node {
                                    margin: UiRect::left(px(style::space::XS)),
                                    ..default()
                                },
                            ));
                        }
                        // A linked model reads as an empty entity otherwise —
                        // its geometry is in the derived subtree, which has no
                        // rows. The badge is how you tell "not flattened yet".
                        if row.linked_model {
                            row_node.spawn((
                                Text::new("linked"),
                                style::no_wrap(),
                                style::mono(&fonts, ui.font_size_xs),
                                TextColor(style::color::TEXT_DIM),
                                Node {
                                    margin: UiRect::left(px(style::space::XS)),
                                    padding: UiRect::axes(px(style::space::XS), px(0.0)),
                                    border: UiRect::all(px(1.0)),
                                    border_radius: BorderRadius::all(px(style::radius::S)),
                                    ..default()
                                },
                                BorderColor::all(style::HAIRLINE),
                            ));
                        }
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
    entities: Query<(
        Entity,
        &SceneId,
        Option<&ChildOf>,
        Option<&Name>,
        Has<editor_scene::models::MeshRef>,
        Has<editor_core::lock::Locked>,
    )>,
    scene_ids: Query<&SceneId>,
    hidden: Res<editor_core::hide::Hidden>,
    body: Query<(&ComputedNode, &ScrollPosition, &PanelBody)>,
    mut state: ResMut<HierarchyState>,
) {
    for (node, scroll, panel) in &body {
        if panel.0.as_str() != HIERARCHY_PANEL || node.size() == Vec2::ZERO {
            continue;
        }
        let view_height = node.size().y * node.inverse_scale_factor();
        let total = build_rows(&entities, &scene_ids, &hidden, &state.collapsed).len();
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

    fn drop_app() -> App {
        let mut app = test_app();
        app.init_resource::<HierarchyDrag>();
        app.init_resource::<editor_core::hide::Hidden>();
        app.add_systems(
            Update,
            perform_hierarchy_drop.in_set(editor_core::EditorSet::Tools),
        );
        app.update();
        app
    }

    fn parent_of(app: &mut App, id: SceneId) -> Option<SceneId> {
        let entity = app.world().resource::<SceneIndex>().get(&id)?;
        let world = app.world_mut();
        let parent = world.get::<ChildOf>(entity)?.parent();
        world.get::<SceneId>(parent).copied()
    }

    /// A drop re-files through the same path `>` and `<` use: one transaction,
    /// one undo entry, real `ChildOf`.
    #[test]
    fn dropping_a_row_onto_another_reparents_it() {
        let mut app = drop_app();
        let (a, b) = (SceneId::random(), SceneId::random());
        spawn_marker(&mut app, a);
        spawn_marker(&mut app, b);
        let depth = app.world().resource::<History>().undo_depth();

        app.world_mut().resource_mut::<HierarchyDrag>().drop = Some((b, Some(a)));
        app.update();
        app.update();

        assert_eq!(parent_of(&mut app, b), Some(a), "the drop did not land");
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth + 1,
            "a drop must be ONE undoable step"
        );
        app.world_mut().resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        assert_eq!(parent_of(&mut app, b), None, "undo did not un-file it");
    }

    /// Dropping onto empty space means the top level — otherwise the only way
    /// to un-parent by mouse would be to drop onto some root and drag out again.
    #[test]
    fn dropping_on_empty_space_lifts_to_the_top_level() {
        let mut app = drop_app();
        let (a, b) = (SceneId::random(), SceneId::random());
        spawn_marker(&mut app, a);
        spawn_marker(&mut app, b);
        app.world_mut().resource_mut::<HierarchyDrag>().drop = Some((b, Some(a)));
        app.update();
        app.update();
        assert_eq!(parent_of(&mut app, b), Some(a));

        app.world_mut().resource_mut::<HierarchyDrag>().drop = Some((b, None));
        app.update();
        app.update();
        assert_eq!(parent_of(&mut app, b), None, "it stayed filed");
    }

    /// THE refusal. A cycle makes those entities unreachable from any root, so
    /// the panel stops listing them and scene capture writes a file without
    /// them — silent loss, from one careless drag.
    #[test]
    fn dropping_a_parent_into_its_own_child_is_refused() {
        let mut app = drop_app();
        let (a, b) = (SceneId::random(), SceneId::random());
        spawn_marker(&mut app, a);
        spawn_marker(&mut app, b);
        app.world_mut().resource_mut::<HierarchyDrag>().drop = Some((b, Some(a)));
        app.update();
        app.update();

        let depth = app.world().resource::<History>().undo_depth();
        // Now try to put `a` inside its own child.
        app.world_mut().resource_mut::<HierarchyDrag>().drop = Some((a, Some(b)));
        app.update();
        app.update();

        assert_eq!(parent_of(&mut app, a), None, "a cycle was created");
        assert_eq!(parent_of(&mut app, b), Some(a), "the tree was disturbed");
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            depth,
            "a refused drop still spent an undo step"
        );
    }

    /// A re-file must not MOVE anything. Reparenting is a change of ownership,
    /// and an object that jumped across the level because you dragged it to a
    /// different row would be a different edit than the one asked for.
    ///
    /// The globals are hand-written: this crate's test app has no
    /// `TransformPlugin`, so nothing propagates and an assertion that trusted
    /// `GlobalTransform` to be computed would pass against anything.
    #[test]
    fn a_dropped_row_keeps_its_place_in_the_world() {
        let mut app = drop_app();
        let (a, b) = (SceneId::random(), SceneId::random());
        spawn_marker(&mut app, a);
        spawn_marker(&mut app, b);

        let (parent_at, child_at) = (Vec3::new(10.0, 0.0, 0.0), Vec3::new(3.0, 0.0, 0.0));
        for (id, at) in [(a, parent_at), (b, child_at)] {
            let entity = app.world().resource::<SceneIndex>().get(&id).unwrap();
            app.world_mut().entity_mut(entity).insert((
                Transform::from_translation(at),
                GlobalTransform::from(Transform::from_translation(at)),
            ));
        }

        app.world_mut().resource_mut::<HierarchyDrag>().drop = Some((b, Some(a)));
        app.update();
        app.update();

        let entity = app.world().resource::<SceneIndex>().get(&b).unwrap();
        let local = app.world().get::<Transform>(entity).unwrap().translation;
        assert_eq!(
            local,
            child_at - parent_at,
            "the child was re-filed but not re-based, so it jumped"
        );
    }
}

#[cfg(test)]
mod virtualization_tests {
    use super::*;

    fn row(id: SceneId, parent: Option<SceneId>) -> Row {
        Row {
            id,
            parent,
            depth: 0,
            has_children: false,
            label: String::new(),
            linked_model: false,
            locked: false,
            hidden: false,
        }
    }

    /// A group dropped inside its own child would make a cycle: those entities
    /// become unreachable from any root, so the panel stops listing them and
    /// scene capture — which walks from roots — writes a file without them.
    #[test]
    fn a_group_cannot_be_dropped_inside_itself() {
        let (a, b, c) = (SceneId::random(), SceneId::random(), SceneId::random());
        let rows = vec![row(a, None), row(b, Some(a)), row(c, Some(b))];
        assert!(is_inside(&rows, a, a), "onto itself is inside itself");
        assert!(is_inside(&rows, a, b), "a direct child is inside");
        assert!(
            is_inside(&rows, a, c),
            "a grandchild is inside — the walk has to go all the way up"
        );
    }

    /// And everything else is fair game, including dropping a parent onto an
    /// unrelated branch.
    #[test]
    fn an_unrelated_row_is_a_valid_target() {
        let (a, b, other) = (SceneId::random(), SceneId::random(), SceneId::random());
        let rows = vec![row(a, None), row(b, Some(a)), row(other, None)];
        assert!(!is_inside(&rows, a, other));
        assert!(
            !is_inside(&rows, b, other),
            "a child can be re-filed anywhere outside its own subtree"
        );
        // A child's PARENT is not inside the child.
        assert!(!is_inside(&rows, b, a));
    }
}
