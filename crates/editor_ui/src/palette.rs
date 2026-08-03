//! Command palette (M1 gate: "open the palette, fuzzy-find an action, invoke it").
//!
//! Lists the `ActionCatalog` (derived — never hand-maintained) with current bindings,
//! filters as you type, Up/Down/Enter/Esc navigation. Applies the M0 spike-3 lessons:
//! `visible_width` on the input (F7), `SelectAllOnFocus`, whole-box focus, and
//! `KeyCapture` so the resolver stands down while typing.
//!
//! The Commands view is SECTIONED (owner rule): true editor-global commands (save,
//! open, play, undo…) lead; commands that belong to a mode are demoted into per-mode
//! groups below — the preferred way to reach them is through their mode.

use bevy::feathers::controls::{FeathersTextInput, FeathersTextInputContainer};
use bevy::feathers::font_styles::InheritableFont;
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::input::keyboard::KeyboardInput;
use bevy::input::ButtonState;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::TextEditChange;
use bevy::ui::px;
use bevy::ui_widgets::SelectAllOnFocus;
use editor_core::prelude::*;

use crate::palette_engine::{rank, PaletteEntry, PaletteItems, PalettePayload};
use crate::style::{self, UiFonts};

/// What this palette is browsing (v1's typed open-modes, spec §7): filters are
/// structural, never query-string hacks.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum PaletteFilter {
    #[default]
    Commands,
    InsertKinds,
    /// Scene search: every named entity, Enter selects (spec: find-object is an
    /// ENGINE INSTANCE, not a bespoke list).
    FindObject,
    /// The material library (C6): Enter assigns to the selection.
    Materials,
    /// The prefab library (M4-D5): Enter places an instance.
    Prefabs,
}

#[derive(Resource, Default)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
    pub filter: PaletteFilter,
}

#[derive(Component, Default, Clone)]
struct PaletteRoot;
#[derive(Component, Default, Clone)]
struct PaletteInput;
#[derive(Component, Default, Clone)]
struct PaletteResults;
#[derive(Component, Default, Clone)]
struct PalettePreview;
#[derive(Component, Default, Clone)]
struct PaletteTitle;
/// The search input container — its font size follows `EditorSettings`.
#[derive(Component, Default, Clone)]
struct PaletteSearchBox;
/// Marks the highlighted result row (scroll-follow target).
#[derive(Component)]
struct SelectedRow;

pub struct PalettePlugin;

impl Plugin for PalettePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PaletteState>()
            .init_resource::<crate::palette_engine::PaletteItems>()
            .add_systems(Startup, spawn_palette)
            .add_systems(
                Update,
                (
                    handle_open_action,
                    build_palette_items,
                    open_on_insert_mode,
                    close_when_editor_leaves,
                    close_when_focus_leaves,
                    update_title,
                    apply_search_font_setting,
                    rebuild_results,
                    scroll_selected_into_view,
                )
                    .chain()
                    .in_set(editor_core::EditorSet::Sync),
            );
    }
}

fn spawn_palette(mut commands: Commands) {
    commands.spawn_scene(bsn! {
        PaletteRoot
        Node {
            position_type: PositionType::Absolute,
            left: percent(50),
            top: px(80),
            margin: UiRect { left: px(-360) },
            width: px(720),
            flex_direction: FlexDirection::Column,
            row_gap: px(style::space::S),
            padding: UiRect::all(px(style::space::S)),
            border: UiRect::all(px(1.0)),
            border_radius: {BorderRadius::all(px(style::radius::L))},
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        BorderColor::all(style::HAIRLINE)
        template_value(style::floating_shadow())
        GlobalZIndex(200)
        Visibility::Hidden
        Children [
            // Mode title (v1 lineage): what this palette is browsing, uppercase, dim.
            (PaletteTitle Text("COMMANDS")
             template(|ctx| Ok(bevy::text::TextFont {
                 font: bevy::text::FontSource::Handle(
                     ctx.resource::<AssetServer>().load(crate::style::SANS_MEDIUM_PATH),
                 ),
                 font_size: bevy::text::FontSize::Px(
                     ctx.resource::<EditorSettings>().ui.font_size_xs,
                 ),
                 ..Default::default()
             }))
             TextColor({crate::style::color::TEXT_DIM})),
            // Search input: larger type than the results list, standard padding.
            (
                @FeathersTextInputContainer
                Node {
                    flex_grow: 0.0,
                    height: px(36),
                    justify_content: JustifyContent::FlexStart,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(style::space::S), px(style::space::XS)),
                }
                PaletteSearchBox
                InheritableFont { font_size: {bevy::text::FontSize::Px(16.0)} }
                on(|_press: On<Pointer<Press>>,
                    inner: Single<Entity, With<PaletteInput>>,
                    mut focus: ResMut<InputFocus>| {
                    focus.set(*inner, FocusCause::Pressed);
                })
                Children [
                    (
                        @FeathersTextInput { @visible_width: 40f32 }
                        PaletteInput
                        SelectAllOnFocus
                        on(update_query)
                        on(palette_keys)
                    )
                ]
            ),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(style::space::S),
                    align_items: AlignItems::Stretch,
                    min_height: px(220.0),
                }
                Children [
                    (PaletteResults Node {
                        flex_direction: FlexDirection::Column,
                        flex_grow: 1.0,
                        row_gap: px(2.0),
                        max_height: px(360.0),
                        overflow: Overflow::scroll_y(),
                    }),
                    (
                        PalettePreview
                        Node {
                            width: px(340),
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(style::space::XS),
                            padding: UiRect::all(px(style::space::S)),
                            border_radius: {BorderRadius::all(px(style::radius::S))},
                        }
                        ThemeBackgroundColor(tokens::PANE_BODY_BG)
                    ),
                ]
            ),
        ]
    });
}

struct DisplayRow {
    label: String,
    suffix: String,
    item: usize,
}

/// One titled group of the results list (None = flat).
struct DisplaySection {
    title: Option<String>,
    rows: Vec<DisplayRow>,
}

fn flat_len(sections: &[DisplaySection]) -> usize {
    sections.iter().map(|s| s.rows.len()).sum()
}

fn flat_get<'a>(sections: &'a [DisplaySection], index: usize) -> Option<&'a DisplayRow> {
    sections.iter().flat_map(|s| &s.rows).nth(index)
}

/// Build the OPEN palette's items (once per open/mode change) — the engine ranks
/// these; nothing below rebuilds them per keystroke.
#[allow(clippy::too_many_arguments)]
fn build_palette_items(
    state: Res<PaletteState>,
    catalog: Res<ActionCatalog>,
    keymap: Res<ResolvedKeymap>,
    modes: Res<Modes>,
    entities: Query<(&SceneId, &Name)>,
    library: Res<editor_scene::materials::MaterialLibrary>,
    prefabs: Res<editor_prefabs::PrefabLibrary>,
    mut edited: MessageReader<Edited>,
    mut items: ResMut<PaletteItems>,
) {
    let scene_changed = edited.read().next().is_some();
    let refresh = state.is_changed()
        || (scene_changed && state.filter == PaletteFilter::FindObject)
        || (library.is_changed() && state.filter == PaletteFilter::Materials)
        || (prefabs.is_changed() && state.filter == PaletteFilter::Prefabs);
    if !refresh {
        return;
    }
    if !state.open {
        items.0.clear();
        return;
    }
    items.0.clear();
    match state.filter {
        PaletteFilter::Prefabs => {
            let mut defs: Vec<_> = prefabs.prefabs.values().collect();
            defs.sort_by(|a, b| a.name.cmp(&b.name));
            for def in defs {
                items.0.push(PaletteEntry {
                    label: def.name.clone(),
                    category: None,
                    keywords: def.id.to_string(),
                    suffix: "prefab".into(),
                    payload: PalettePayload::Prefab(def.id),
                });
            }
        }
        PaletteFilter::Materials => {
            for def in &library.materials {
                items.0.push(PaletteEntry {
                    label: def.name.clone(),
                    category: None,
                    keywords: def.id.to_string(),
                    suffix: "material".into(),
                    payload: PalettePayload::Material(def.id),
                });
            }
            // Creating one belongs in the same surface.
            items.0.push(PaletteEntry {
                label: "New Material…".into(),
                category: None,
                keywords: "create material new".into(),
                suffix: String::new(),
                payload: PalettePayload::Action(ActionId::new_static("material.new")),
            });
        }
        PaletteFilter::FindObject => {
            let mut named: Vec<(&SceneId, &Name)> = entities.iter().collect();
            named.sort_by_key(|(id, _)| id.0);
            for (id, name) in named {
                items.0.push(PaletteEntry {
                    label: name.as_str().to_string(),
                    category: None,
                    keywords: id.0.to_string(),
                    suffix: id.0.to_string()[..4].to_string(),
                    payload: PalettePayload::Entity(*id),
                });
            }
        }
        PaletteFilter::InsertKinds => {
            for def in &catalog.actions {
                if def.flags.hidden || !def.id.as_str().starts_with("insert.kind.") {
                    continue;
                }
                items.0.push(entry_for_action(def, &keymap, None));
            }
            items.0.sort_by(|a, b| a.label.cmp(&b.label));
        }
        PaletteFilter::Commands => {
            // EDITOR block first, then per-mode blocks (owner rule) — the engine
            // preserves this order for empty queries; searching goes flat-ranked.
            let mut editor = Vec::new();
            let mut modal: std::collections::BTreeMap<String, Vec<PaletteEntry>> =
                Default::default();
            for def in &catalog.actions {
                if def.flags.hidden {
                    continue;
                }
                let section = def.contexts.iter().find_map(|context| {
                    if context.as_str() == "normal" {
                        return None;
                    }
                    modes
                        .get(&ModeId::new(context.as_str().to_string()))
                        .map(|mode| mode.name.to_uppercase())
                });
                match section {
                    Some(section) => modal
                        .entry(section.clone())
                        .or_default()
                        .push(entry_for_action(def, &keymap, Some(section))),
                    None => editor.push(entry_for_action(def, &keymap, Some("EDITOR".into()))),
                }
            }
            editor.sort_by(|a, b| a.label.cmp(&b.label));
            items.0.extend(editor);
            for (_, mut block) in modal {
                block.sort_by(|a, b| a.label.cmp(&b.label));
                items.0.extend(block);
            }
        }
    }
}

fn entry_for_action(
    def: &editor_api::actions::ActionDef,
    keymap: &ResolvedKeymap,
    category: Option<String>,
) -> PaletteEntry {
    let suffix = keymap
        .by_context
        .values()
        .flatten()
        .find(|(_, action)| action == &def.id)
        .map(|(binding, _)| style::pretty_binding(binding))
        .unwrap_or_default();
    PaletteEntry {
        label: def.name.to_string(),
        category,
        keywords: format!("{} {}", def.id.as_str(), def.description),
        suffix,
        payload: PalettePayload::Action(def.id.clone()),
    }
}

/// Ranked view: empty query keeps category grouping (source order); a live query
/// flattens to best-match-first (what fingers expect from fuzzy search).
fn display_sections(
    items: &PaletteItems,
    query: &str,
    max_results: usize,
) -> Vec<DisplaySection> {
    let ranked = rank(&items.0, query);
    let mut sections: Vec<DisplaySection> = Vec::new();
    let mut total = 0usize;
    for index in ranked {
        if total >= max_results {
            break;
        }
        let item = &items.0[index];
        let row = DisplayRow {
            label: item.label.clone(),
            suffix: item.suffix.clone(),
            item: index,
        };
        let title = if query.is_empty() { item.category.clone() } else { None };
        match sections.last_mut() {
            Some(section) if section.title == title => section.rows.push(row),
            _ => sections.push(DisplaySection { title, rows: vec![row] }),
        }
        total += 1;
    }
    sections
}

fn open_palette(
    state: &mut PaletteState,
    capture: &mut KeyCapture,
    focus: &mut InputFocus,
    input: Entity,
    root_vis: &mut Visibility,
) {
    state.open = true;
    state.selected = 0;
    state.query.clear();
    capture.0 = true;
    focus.set(input, FocusCause::Navigated);
    *root_vis = Visibility::Visible;
}

fn close_palette(
    state: &mut PaletteState,
    capture: &mut KeyCapture,
    focus: &mut InputFocus,
    root_vis: &mut Visibility,
) {
    state.open = false;
    state.filter = PaletteFilter::Commands;
    capture.0 = false;
    focus.clear();
    *root_vis = Visibility::Hidden;
}

/// Entering insert mode auto-opens the palette prefilled with the insert query so
/// you search kinds immediately (owner direction).
fn open_on_insert_mode(
    mut modes: MessageReader<ModeChanged>,
    mut just_picked: ResMut<editor_core::prelude::KindJustPicked>,
    mut state: ResMut<PaletteState>,
    mut capture: ResMut<KeyCapture>,
    mut focus: ResMut<InputFocus>,
    input: Single<Entity, With<PaletteInput>>,
    mut editable: Query<&mut bevy::text::EditableText>,
    mut root: Single<&mut Visibility, With<PaletteRoot>>,
) {
    for change in modes.read() {
        // Entering insert ALWAYS offers the palette (owner: search right away, every
        // time) — except when the entry was CAUSED by a palette kind pick, which
        // must reveal the scene + fresh ghost instead of re-covering them.
        let picked = std::mem::take(&mut just_picked.0);
        if change.to == editor_core::prelude::MODE_INSERT && !state.open && !picked {
            open_palette(&mut state, &mut capture, &mut focus, *input, &mut root);
            state.filter = PaletteFilter::InsertKinds;
            if let Ok(mut text) = editable.get_mut(*input) {
                text.clear();
            }
        }
    }
}

/// Filter-aware title (the v1-lineage header): what this palette is browsing.
fn update_title(state: Res<PaletteState>, mut title: Query<&mut Text, With<PaletteTitle>>) {
    if !state.is_changed() {
        return;
    }
    let label = match state.filter {
        PaletteFilter::InsertKinds => "INSERT OBJECT",
        PaletteFilter::Commands => "COMMANDS",
        PaletteFilter::FindObject => "FIND OBJECT",
        PaletteFilter::Materials => "ASSIGN MATERIAL",
        PaletteFilter::Prefabs => "PLACE PREFAB",
    };
    for mut text in &mut title {
        if text.0 != label {
            text.0 = label.to_string();
        }
    }
}

fn handle_open_action(
    mut reader: MessageReader<ActionInvoked>,
    mode: Res<CurrentMode>,
    mut state: ResMut<PaletteState>,
    mut capture: ResMut<KeyCapture>,
    mut focus: ResMut<InputFocus>,
    input: Single<Entity, With<PaletteInput>>,
    mut editable: Query<&mut bevy::text::EditableText>,
    mut root: Single<&mut Visibility, With<PaletteRoot>>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() == "core.palette" && !state.open {
            open_palette(&mut state, &mut capture, &mut focus, *input, &mut root);
            // Contextual filter: the palette opened in insert mode browses kinds.
            state.filter = if mode.0 == editor_core::prelude::MODE_INSERT {
                PaletteFilter::InsertKinds
            } else {
                PaletteFilter::Commands
            };
            if let Ok(mut text) = editable.get_mut(*input) {
                text.clear();
            }
        }
        if invoked.action.as_str() == "prefab.place" && !state.open {
            open_palette(&mut state, &mut capture, &mut focus, *input, &mut root);
            state.filter = PaletteFilter::Prefabs;
            if let Ok(mut text) = editable.get_mut(*input) {
                text.clear();
            }
        }
        if invoked.action.as_str() == "material.assign" && !state.open {
            open_palette(&mut state, &mut capture, &mut focus, *input, &mut root);
            state.filter = PaletteFilter::Materials;
            if let Ok(mut text) = editable.get_mut(*input) {
                text.clear();
            }
        }
        if invoked.action.as_str() == "core.find-object" && !state.open {
            open_palette(&mut state, &mut capture, &mut focus, *input, &mut root);
            state.filter = PaletteFilter::FindObject;
            if let Ok(mut text) = editable.get_mut(*input) {
                text.clear();
            }
        }
        // Escape always backs out (even when it pierced key capture).
        if invoked.action.as_str() == "core.escape-home" && state.open {
            close_palette(&mut state, &mut capture, &mut focus, &mut root);
        }
    }
}

/// Standard modal behavior: focus leaving the palette input (click-away, focus steal)
/// closes it — otherwise KeyCapture would trap the keyboard with no visible owner.
fn close_when_focus_leaves(
    input: Single<Entity, With<PaletteInput>>,
    mut state: ResMut<PaletteState>,
    mut capture: ResMut<KeyCapture>,
    mut focus: ResMut<InputFocus>,
    mut root: Single<&mut Visibility, With<PaletteRoot>>,
) {
    if state.open && focus.get() != Some(*input) {
        close_palette(&mut state, &mut capture, &mut focus, &mut root);
    }
}

/// F12 (or anything) deactivating the editor closes the palette with it.
fn close_when_editor_leaves(
    editor: Res<EditorState>,
    mut state: ResMut<PaletteState>,
    mut capture: ResMut<KeyCapture>,
    mut focus: ResMut<InputFocus>,
    mut root: Single<&mut Visibility, With<PaletteRoot>>,
) {
    if state.open && !editor.active {
        close_palette(&mut state, &mut capture, &mut focus, &mut root);
    }
}

fn update_query(
    _change: On<TextEditChange>,
    text: Single<&bevy::text::EditableText, With<PaletteInput>>,
    mut state: ResMut<PaletteState>,
) {
    let value = text.value().to_string();
    if state.open && state.query != value {
        state.query = value;
        state.selected = 0;
    }
}

/// Palette navigation, observed on the focused input (the widgets idiom — no raw
/// `ButtonInput` outside the resolver).
fn palette_keys(
    mut event: On<FocusedInput<KeyboardInput>>,
    items: Res<PaletteItems>,
    index: Res<SceneIndex>,
    settings: Res<EditorSettings>,
    mut state: ResMut<PaletteState>,
    mut capture: ResMut<KeyCapture>,
    mut focus: ResMut<InputFocus>,
    mut root: Single<&mut Visibility, With<PaletteRoot>>,
    mut actions: MessageWriter<ActionInvoked>,
    mut commands: Commands,
) {
    if !state.open || event.input.state != ButtonState::Pressed {
        return;
    }
    let sections = display_sections(&items, &state.query, settings.ui.palette_max_results);
    let result_count = flat_len(&sections);
    match event.input.key_code {
        KeyCode::ArrowDown => {
            if result_count > 0 {
                state.selected = (state.selected + 1).min(result_count - 1);
            }
            event.propagate(false);
        }
        KeyCode::ArrowUp => {
            state.selected = state.selected.saturating_sub(1);
            event.propagate(false);
        }
        KeyCode::Enter => {
            if let Some(row) = flat_get(&sections, state.selected) {
                match &items.0[row.item].payload {
                    PalettePayload::Action(action) => {
                        actions.write(ActionInvoked {
                            action: action.clone(),
                            args: None,
                            source: InvocationSource::Palette,
                        });
                    }
                    PalettePayload::Prefab(prefab) => {
                        let prefab = *prefab;
                        commands.queue(move |world: &mut World| {
                            // Place at the cursor's ground point when available.
                            let at = world
                                .resource::<CursorGround>()
                                .0
                                .unwrap_or(Vec3::ZERO);
                            world.resource_mut::<EditQueue>().0.push(Transaction {
                                label: "Place Prefab".into(),
                                gesture: None,
                                ops: vec![Op::Spawn {
                                    id: SceneId::random(),
                                    components: vec![
                                        Box::new(editor_prefabs::PrefabInstance(prefab))
                                            .into_partial_reflect(),
                                        Box::new(editor_prefabs::PrefabOverrides::default())
                                            .into_partial_reflect(),
                                        Box::new(Transform::from_translation(at))
                                            .into_partial_reflect(),
                                        Box::new(Name::new("Prefab Instance"))
                                            .into_partial_reflect(),
                                    ],
                                }],
                            });
                        });
                    }
                    PalettePayload::Material(material) => {
                        let material = *material;
                        commands.queue(move |world: &mut World| {
                            let selected: Vec<SceneId> = {
                                let mut query = world
                                    .query_filtered::<&SceneId, With<Selected>>();
                                query.iter(world).copied().collect()
                            };
                            if selected.is_empty() {
                                return;
                            }
                            // ONE transaction for the whole selection (C6).
                            let ops = selected
                                .into_iter()
                                .map(|target| Op::Set {
                                    target,
                                    value: Box::new(
                                        editor_scene::materials::MaterialRef(material),
                                    )
                                    .into_partial_reflect(),
                                })
                                .collect::<Vec<_>>();
                            world.resource_mut::<EditQueue>().0.push(Transaction {
                                label: "Assign Material".into(),
                                gesture: None,
                                ops,
                            });
                        });
                    }
                    PalettePayload::Entity(id) => {
                        if let Some(entity) = index.get(id) {
                            commands.queue(move |world: &mut World| {
                                let previous: Vec<Entity> = world
                                    .query_filtered::<Entity, With<Selected>>()
                                    .iter(world)
                                    .collect();
                                for entity in previous {
                                    world.entity_mut(entity).remove::<Selected>();
                                }
                                world.entity_mut(entity).insert(Selected);
                                world.write_message(SelectionChanged);
                            });
                        }
                    }
                }
            }
            close_palette(&mut state, &mut capture, &mut focus, &mut root);
            event.propagate(false);
        }
        KeyCode::Escape => {
            close_palette(&mut state, &mut capture, &mut focus, &mut root);
            event.propagate(false);
        }
        _ => {}
    }
}

/// Keyboard-first: arrow navigation must never walk the highlight out of the
/// scrollable viewport. Reads the laid-out geometry (one frame behind a rebuild,
/// imperceptible) and clamps the container's scroll so the row stays visible.
fn scroll_selected_into_view(
    container: Single<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        With<PaletteResults>,
    >,
    row: Option<
        Single<(&ComputedNode, &UiGlobalTransform), (With<SelectedRow>, Without<PaletteResults>)>,
    >,
) {
    let Some(row) = row else { return };
    let (cont_node, cont_tf, mut scroll) = container.into_inner();
    let (row_node, row_tf) = *row;
    // A row spawned THIS frame has default (zeroed) geometry until layout runs —
    // using it would corrupt the scroll. Follow next frame instead.
    if row_node.size() == Vec2::ZERO {
        return;
    }
    let scale = cont_node.inverse_scale_factor();
    let cont_h = cont_node.size().y * scale;
    let row_h = row_node.size().y * scale;
    // UiGlobalTransform.translation = node CENTER, physical px, y-down (0.19 —
    // plain GlobalTransform stays identity for UI nodes).
    // Row top in content coordinates (logical px): visible offset plus current scroll.
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

/// The search box's type size follows settings (applied post-spawn because the
/// feathers container already carries an `InheritableFont` — a template patch on the
/// same component would DUPLICATE it in the spawn bundle, which panics; a static
/// patch merges, then this system owns the live value).
fn apply_search_font_setting(
    settings: Res<EditorSettings>,
    search: Option<Single<&mut InheritableFont, With<PaletteSearchBox>>>,
) {
    if !settings.is_changed() {
        return;
    }
    let Some(mut font) = search.map(Single::into_inner) else { return };
    font.font_size = bevy::text::FontSize::Px(settings.ui.font_size_search);
}

fn rebuild_results(
    state: Res<PaletteState>,
    items: Res<PaletteItems>,
    catalog: Res<ActionCatalog>,
    keymap: Res<ResolvedKeymap>,
    settings: Res<EditorSettings>,
    results: Single<Entity, With<PaletteResults>>,
    preview: Single<Entity, With<PalettePreview>>,
    fonts: Res<UiFonts>,
    mut commands: Commands,
) {
    if !state.is_changed() {
        return;
    }
    commands.entity(*results).despawn_related::<Children>();
    commands.entity(*preview).despawn_related::<Children>();
    if !state.open {
        return;
    }
    let ui = settings.ui.clone();
    let sections = display_sections(&items, &state.query, settings.ui.palette_max_results);

    // Left pane: the sectioned result list. Headers are chrome, not results —
    // selection indexes action rows only and skips straight over them.
    commands.entity(*results).with_children(|parent| {
        let mut flat_index = 0usize;
        for (section_index, section) in sections.iter().enumerate() {
            if let Some(title) = &section.title {
                parent.spawn((
                    Text::new(title.clone()),
                    style::sans_medium(&fonts, ui.font_size_xs),
                    TextColor(style::color::TEXT_DIM),
                    Node {
                        padding: UiRect::horizontal(px(style::space::S)),
                        margin: UiRect::top(px(if section_index == 0 {
                            2.0
                        } else {
                            style::space::M
                        })),
                        flex_shrink: 0.0,
                        ..default()
                    },
                ));
            }
            for row in &section.rows {
                let selected = flat_index == state.selected;
                flat_index += 1;
                let mut entity = parent.spawn((
                    Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(px(style::space::S), px(style::space::XS)),
                        column_gap: px(style::space::M),
                        border_radius: BorderRadius::all(px(style::radius::S)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(if selected {
                        style::color::selection()
                    } else {
                        Color::NONE
                    }),
                ));
                if selected {
                    entity.insert(SelectedRow);
                }
                entity.with_children(|row_node| {
                    row_node.spawn((
                        Text::new(row.label.clone()),
                        style::sans(&fonts, ui.font_size_m),
                    ));
                    if !row.suffix.is_empty() {
                        row_node.spawn((
                            Text::new(row.suffix.clone()),
                            style::mono(&fonts, ui.font_size_s),
                            TextColor(style::color::TEXT_KEYS),
                        ));
                    }
                });
            }
        }
        if flat_index == 0 {
            parent.spawn((
                Text::new("no matching actions"),
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
                Node { padding: UiRect::all(px(style::space::S)), ..default() },
            ));
        }
    });

    // Right pane: preview/docs for the selection — action docs for commands,
    // identity for entities (the same surface previews assets/materials later).
    let selected_payload =
        flat_get(&sections, state.selected).map(|row| items.0[row.item].payload.clone());
    let selected_label = flat_get(&sections, state.selected).map(|row| row.label.clone());
    let selected_def = match &selected_payload {
        Some(PalettePayload::Action(id)) => catalog.get(id).cloned(),
        _ => None,
    };
    commands.entity(*preview).with_children(|pane| {
        if let Some(PalettePayload::Entity(id)) = &selected_payload {
            pane.spawn((
                Text::new(selected_label.unwrap_or_default()),
                style::sans_medium(&fonts, ui.font_size_m),
            ));
            pane.spawn((
                Text::new(id.0.to_string()),
                style::mono(&fonts, ui.font_size_xs),
                TextColor(style::color::TEXT_DIM),
            ));
            pane.spawn((
                Text::new("⏎ select in viewport"),
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
                Node { margin: UiRect::top(px(style::space::XS)), ..default() },
            ));
            return;
        }
        let Some(def) = selected_def else {
            pane.spawn((
                Text::new("no selection"),
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
            ));
            return;
        };
        pane.spawn((Text::new(def.name.to_string()), style::sans_medium(&fonts, ui.font_size_m)));
        pane.spawn((
            Text::new(def.id.to_string()),
            style::mono(&fonts, ui.font_size_xs),
            TextColor(style::color::TEXT_DIM),
        ));
        if !def.description.is_empty() {
            pane.spawn((
                Text::new(def.description.to_string()),
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
                Node { margin: UiRect::top(px(style::space::XS)), ..default() },
            ));
        }
        let bindings: Vec<String> = keymap
            .by_context
            .iter()
            .flat_map(|(context, entries)| {
                entries.iter().filter(|(_, action)| action == &def.id).map(move |(binding, _)| {
                    format!("{}  ·  {}", style::pretty_binding(binding), context)
                })
            })
            .collect();
        if !bindings.is_empty() {
            pane.spawn((
                Text::new("BINDINGS"),
                style::sans(&fonts, ui.font_size_xs),
                TextColor(style::color::TEXT_DIM),
                Node { margin: UiRect::top(px(style::space::S)), ..default() },
            ));
            for line in bindings {
                pane.spawn((
                    Text::new(line),
                    style::mono(&fonts, ui.font_size_s),
                    TextColor(style::color::TEXT_KEYS),
                ));
            }
        }
    });
}
