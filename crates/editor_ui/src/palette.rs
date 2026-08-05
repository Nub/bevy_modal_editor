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
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::input_focus::{FocusCause, FocusedInput, InputFocus};
use bevy::prelude::*;
use bevy::text::TextEditChange;
use bevy::ui::px;
use bevy::ui_widgets::SelectAllOnFocus;
use editor_core::prelude::*;

use crate::palette_engine::{PaletteEntry, PaletteItems, PalettePayload, rank};
use crate::style::{self, UiFonts};

/// What this palette is browsing (v1's typed open-modes, spec §7): filters are
/// structural, never query-string hacks.
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum PaletteFilter {
    /// `i` with a selection: add a component to the selected entities.
    AddComponent,
    /// `/` with a selection: find a component ON the selection, jump to it.
    ComponentSearch,
    #[default]
    Commands,
    InsertKinds,
    /// Scene search: every named entity, Enter selects (spec: find-object is an
    /// ENGINE INSTANCE, not a bespoke list).
    FindObject,
    /// The material library (C6): Enter assigns to the selection.
    Materials,
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
                    open_on_insert_mode,
                    close_when_editor_leaves,
                    close_when_focus_leaves,
                    build_palette_items,
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
        bevy::ui::UiTransform::IDENTITY
        crate::appear::FloatingSurface
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
                        bevy::ui_widgets::ScrollArea
                        Node {
                            width: px(340),
                            max_height: px(360.0),
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(style::space::XS),
                            padding: UiRect::all(px(style::space::S)),
                            border_radius: {BorderRadius::all(px(style::radius::S))},
                            overflow: Overflow::scroll_y(),
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

fn flat_get(sections: &[DisplaySection], index: usize) -> Option<&DisplayRow> {
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
    models: Res<editor_scene::models::ModelLibrary>,
    components: Res<editor_core::edits::EditorComponents>,
    registry: Res<AppTypeRegistry>,
    inspector_model: Res<crate::inspector::InspectorModel>,
    settings: Res<EditorSettings>,
    mut edited: MessageReader<Edited>,
    mut items: ResMut<PaletteItems>,
) {
    let scene_changed = edited.read().next().is_some();
    let refresh = state.is_changed()
        || (scene_changed && state.filter == PaletteFilter::FindObject)
        || (library.is_changed() && state.filter == PaletteFilter::Materials)
        || ((prefabs.is_changed() || models.is_changed())
            && state.filter == PaletteFilter::InsertKinds)
        || (settings.is_changed() && state.filter == PaletteFilter::AddComponent);
    if !refresh {
        return;
    }
    if !state.open {
        items.0.clear();
        return;
    }
    items.0.clear();
    match state.filter {
        PaletteFilter::AddComponent => {
            // EVERY reflectable component in the registry (owner: full
            // reflection, like v1). SERIALIZED ones (feature-registered) rank
            // in their own section first; the rest is the full surface.
            // Non-defaultable ones show "(no default)" and refuse politely.
            let registered: std::collections::HashSet<std::any::TypeId> =
                components.types.iter().map(|r| r.type_id).collect();
            let registry = registry.read();
            let mut entries: Vec<(bool, String, PaletteEntry)> = Vec::new();
            for registration in registry.iter() {
                if registration
                    .data::<bevy::ecs::reflect::ReflectComponent>()
                    .is_none()
                {
                    continue;
                }
                let info = registration.type_info();
                let type_path = info.type_path();
                // Generic-aware short name — naive `rsplit("::")` leaves
                // dangling brackets on generics ("SpriteMaterial>").
                let short = info.type_path_table().short_path();
                let has_default = registration
                    .data::<bevy::reflect::std_traits::ReflectDefault>()
                    .is_some();
                let serialized = registered.contains(&info.type_id());
                let docs = info.docs().unwrap_or_default();
                entries.push((
                    serialized,
                    short.to_string(),
                    PaletteEntry {
                        label: short.to_string(),
                        category: Some(
                            if serialized {
                                "SERIALIZED"
                            } else {
                                "ALL COMPONENTS"
                            }
                            .into(),
                        ),
                        keywords: format!("{type_path} {docs}"),
                        suffix: if has_default {
                            String::new()
                        } else {
                            "(no default)".into()
                        },
                        payload: PalettePayload::AddComponent(info.type_id()),
                    },
                ));
            }
            entries.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            items.0.extend(entries.into_iter().map(|(_, _, e)| e));
        }
        PaletteFilter::ComponentSearch => {
            // Components ON the selection = the inspector's live section list.
            for row in &inspector_model.rows {
                if let crate::inspector::RowSpec::Section(title) = row {
                    items.0.push(PaletteEntry {
                        label: title.clone(),
                        category: Some("ON SELECTION".into()),
                        keywords: String::new(),
                        suffix: String::new(),
                        payload: PalettePayload::RevealComponent(title.clone()),
                    });
                }
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
            // Rows read as the THING, not the verb — the palette title
            // already says INSERT ("Sphere", never "Insert: Sphere").
            // ONE insert surface (owner): everything placeable — registered
            // kinds AND library prefabs, grouped.
            // PREFABS first: on a ranking tie (a prefab named like a kind,
            // e.g. "cube" vs Cube), the user's authored content wins over the
            // built-in primitive — source order is the fuzzy tiebreak.
            let mut defs: Vec<_> = prefabs.prefabs.values().collect();
            defs.sort_by(|a, b| a.name.cmp(&b.name));
            for def in defs {
                items.0.push(PaletteEntry {
                    label: def.name.clone(),
                    category: Some("PREFABS".into()),
                    keywords: def.id.to_string(),
                    suffix: "prefab".into(),
                    payload: PalettePayload::Prefab(def.id),
                });
            }
            // MODELS: imported sources (D12) — placing references BY UUID.
            let mut model_entries: Vec<_> = models.entries.iter().collect();
            model_entries.sort_by(|a, b| a.name.cmp(&b.name));
            for entry in model_entries {
                items.0.push(PaletteEntry {
                    label: entry.name.clone(),
                    category: Some("MODELS".into()),
                    keywords: format!("{} {}", entry.asset_path, entry.uuid),
                    suffix: "model".into(),
                    payload: PalettePayload::Model(entry.uuid),
                });
            }
            let mut kinds = Vec::new();
            for def in &catalog.actions {
                if def.flags.hidden || !def.id.as_str().starts_with("insert.kind.") {
                    continue;
                }
                let mut entry = entry_for_action(def, &keymap, Some("OBJECTS".into()));
                if let Some(bare) = entry.label.strip_prefix("Insert: ") {
                    entry.label = bare.to_string();
                }
                kinds.push(entry);
            }
            kinds.sort_by(|a, b| a.label.cmp(&b.label));
            items.0.extend(kinds);
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
fn display_sections(items: &PaletteItems, query: &str, max_results: usize) -> Vec<DisplaySection> {
    let ranked = rank(&items.0, query);
    let mut sections: Vec<DisplaySection> = Vec::new();
    for index in ranked.into_iter().take(max_results) {
        let item = &items.0[index];
        let row = DisplayRow {
            label: item.label.clone(),
            suffix: item.suffix.clone(),
            item: index,
        };
        let title = if query.is_empty() {
            item.category.clone()
        } else {
            None
        };
        match sections.last_mut() {
            Some(section) if section.title == title => section.rows.push(row),
            _ => sections.push(DisplaySection {
                title,
                rows: vec![row],
            }),
        }
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
    selection: Query<(), With<Selected>>,
    mut commands: Commands,
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
            // `i` while HOLDING a selection means "insert INTO what I hold":
            // add-component palette, and insert mode never engages (owner).
            if !selection.is_empty() {
                // Deferred: this system READS ModeChanged, so the reset must not
                // also write it inline (B0002).
                commands.queue(|world: &mut World| {
                    let changed = {
                        let mut mode = world.resource_mut::<CurrentMode>();
                        (mode.0 != editor_core::prelude::MODE_NORMAL).then(|| {
                            std::mem::replace(&mut mode.0, editor_core::prelude::MODE_NORMAL)
                        })
                    };
                    if let Some(from) = changed {
                        world.write_message(ModeChanged {
                            from,
                            to: editor_core::prelude::MODE_NORMAL,
                        });
                    }
                });
                open_palette(&mut state, &mut capture, &mut focus, *input, &mut root);
                state.filter = PaletteFilter::AddComponent;
                if let Ok(mut text) = editable.get_mut(*input) {
                    text.clear();
                }
                continue;
            }
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
        PaletteFilter::AddComponent => "ADD COMPONENT",
        PaletteFilter::ComponentSearch => "COMPONENTS ON SELECTION",
        PaletteFilter::Commands => "COMMANDS",
        PaletteFilter::FindObject => "FIND OBJECT",
        PaletteFilter::Materials => "ASSIGN MATERIAL",
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
    selection: Query<(), With<Selected>>,
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
        if invoked.action.as_str() == "material.assign" && !state.open {
            open_palette(&mut state, &mut capture, &mut focus, *input, &mut root);
            state.filter = PaletteFilter::Materials;
            if let Ok(mut text) = editable.get_mut(*input) {
                text.clear();
            }
        }
        if invoked.action.as_str() == "core.find-object" && !state.open {
            open_palette(&mut state, &mut capture, &mut focus, *input, &mut root);
            // `/` is search: the scene when hands are empty, the SELECTION's
            // components when holding something (owner: rapid editing).
            state.filter = if selection.is_empty() {
                PaletteFilter::FindObject
            } else {
                PaletteFilter::ComponentSearch
            };
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
        info!(
            "palette closed: focus left the input (now {:?})",
            focus.get()
        );
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
    keys: Res<ButtonInput<KeyCode>>,
    mut settings: ResMut<EditorSettings>,
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
        // ⌃f: pin/unpin the highlighted component (owner: favorites on top).
        KeyCode::KeyF
            if keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight) =>
        {
            if state.filter == PaletteFilter::AddComponent {
                let sections =
                    display_sections(&items, &state.query, settings.ui.palette_max_results);
                if let Some(row) = flat_get(&sections, state.selected)
                    && let PalettePayload::AddComponent(_) = &items.0[row.item].payload
                {
                    let type_path = items.0[row.item]
                        .keywords
                        .split(' ')
                        .next()
                        .unwrap_or_default()
                        .to_string();
                    settings.toggle_favorite_component(&type_path);
                }
            }
            event.propagate(false);
        }
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
            debug!(
                "palette Enter: filter={:?} query={:?} selected={} rows={}",
                state.filter, state.query, state.selected, result_count
            );
            if let Some(row) = flat_get(&sections, state.selected) {
                match &items.0[row.item].payload {
                    PalettePayload::Action(action) => {
                        actions.write(ActionInvoked {
                            action: action.clone(),
                            args: None,
                            source: InvocationSource::Palette,
                        });
                    }
                    PalettePayload::AddComponent(type_id) => {
                        let type_id = *type_id;
                        commands.queue(move |world: &mut World| {
                            let selected: Vec<SceneId> = {
                                let mut query = world.query_filtered::<&SceneId, With<Selected>>();
                                query.iter(world).copied().collect()
                            };
                            if selected.is_empty() {
                                return;
                            }
                            let registry_arc = world.resource::<AppTypeRegistry>().clone();
                            let registry = registry_arc.read();
                            let Some(registration) = registry.get(type_id) else {
                                return;
                            };
                            let Some(reflect_default) =
                                registration.data::<bevy::reflect::std_traits::ReflectDefault>()
                            else {
                                return;
                            };
                            let short = registration
                                .type_info()
                                .type_path_table()
                                .short_path()
                                .to_string();
                            let count = selected.len();
                            // ONE undoable transaction across the whole selection.
                            let ops = selected
                                .into_iter()
                                .map(|target| Op::Set {
                                    target,
                                    value: reflect_default.default().into_partial_reflect(),
                                })
                                .collect();
                            drop(registry);
                            world.resource_mut::<EditQueue>().0.push(Transaction {
                                label: format!("Add {short}"),
                                gesture: None,
                                ops,
                            });
                            world.write_message(editor_scene::SceneIoFeedback {
                                message: format!("{short} added to {count} selected"),
                                success: true,
                            });
                        });
                    }
                    PalettePayload::RevealComponent(section) => {
                        let section = section.clone();
                        commands.queue(move |world: &mut World| {
                            world.resource_mut::<PanelFocus>().0 =
                                Some(PanelId::new_static("inspector"));
                            world.resource_mut::<crate::inspector::InspectorReveal>().0 =
                                Some(section);
                        });
                    }
                    PalettePayload::Prefab(prefab) => {
                        let prefab = *prefab;
                        commands.queue(move |world: &mut World| {
                            // The palette covers the cursor (CursorGround is None
                            // over chrome), so fall back to the ground point the
                            // CAMERA is looking at — placement must land where
                            // the user is looking, never invisibly at origin.
                            let at = world
                                .resource::<CursorGround>()
                                .0
                                .or_else(|| camera_focus_ground(world))
                                .unwrap_or(Vec3::ZERO);
                            let (name, def_sockets) = {
                                let library = world.resource::<editor_prefabs::PrefabLibrary>();
                                let def = library.prefabs.get(&prefab);
                                (
                                    def.map(|p| p.name.clone())
                                        .unwrap_or_else(|| "prefab".into()),
                                    def.map(editor_prefabs::sockets::template_sockets)
                                        .unwrap_or_default(),
                                )
                            };
                            // D9: a compatible socket near the cursor wins over
                            // the raw ground point — pieces MATE.
                            let snap = editor_prefabs::sockets::snap_for_placement(
                                world,
                                &def_sockets,
                                at,
                                3.0,
                            );
                            let id = SceneId::random();
                            world.resource_mut::<EditQueue>().0.push(Transaction {
                                label: "Place Prefab".into(),
                                gesture: None,
                                ops: vec![Op::Spawn {
                                    id,
                                    components: vec![
                                        Box::new(editor_prefabs::PrefabInstance(prefab))
                                            .into_partial_reflect(),
                                        Box::new(editor_prefabs::PrefabOverrides::default())
                                            .into_partial_reflect(),
                                        Box::new(Transform::from_translation(at))
                                            .into_partial_reflect(),
                                        Box::new(Name::new(name.clone())).into_partial_reflect(),
                                    ],
                                }],
                            });
                            // Placement must be SEEN: select it (outline +
                            // inspector), drop back to normal mode, say so.
                            world
                                .resource_mut::<editor_prefabs::authoring::PendingGroupSelect>()
                                .0 = Some(id);
                            let from = {
                                let mut current = world.resource_mut::<CurrentMode>();
                                (current.0 != MODE_NORMAL)
                                    .then(|| std::mem::replace(&mut current.0, MODE_NORMAL))
                            };
                            if let Some(from) = from {
                                world.write_message(ModeChanged {
                                    from,
                                    to: MODE_NORMAL,
                                });
                            }
                            let message = match &snap {
                                Some((_, label)) => {
                                    format!("placed \u{25c6} {name} \u{b7} {label}")
                                }
                                None => format!("placed \u{25c6} {name}"),
                            };
                            world.write_message(editor_scene::SceneIoFeedback {
                                message,
                                success: true,
                            });
                        });
                    }
                    PalettePayload::Model(model) => {
                        let model = *model;
                        commands.queue(move |world: &mut World| {
                            let at = world
                                .resource::<CursorGround>()
                                .0
                                .or_else(|| camera_focus_ground(world))
                                .unwrap_or(Vec3::ZERO);
                            let name = world
                                .resource::<editor_scene::models::ModelLibrary>()
                                .get(&model)
                                .map(|entry| entry.name.clone())
                                .unwrap_or_else(|| "model".into());
                            let id = SceneId::random();
                            world.resource_mut::<EditQueue>().0.push(Transaction {
                                label: "Place Model".into(),
                                gesture: None,
                                ops: vec![Op::Spawn {
                                    id,
                                    components: vec![
                                        Box::new(editor_scene::models::MeshRef(model))
                                            .into_partial_reflect(),
                                        Box::new(Transform::from_translation(at))
                                            .into_partial_reflect(),
                                        Box::new(Name::new(name.clone())).into_partial_reflect(),
                                    ],
                                }],
                            });
                            world
                                .resource_mut::<editor_core::selection::PendingSelect>()
                                .0 = Some(id);
                            let from = {
                                let mut current = world.resource_mut::<CurrentMode>();
                                (current.0 != MODE_NORMAL)
                                    .then(|| std::mem::replace(&mut current.0, MODE_NORMAL))
                            };
                            if let Some(from) = from {
                                world.write_message(ModeChanged {
                                    from,
                                    to: MODE_NORMAL,
                                });
                            }
                            world.write_message(editor_scene::SceneIoFeedback {
                                message: format!("placed {name}"),
                                success: true,
                            });
                        });
                    }
                    PalettePayload::Material(material) => {
                        let material = *material;
                        commands.queue(move |world: &mut World| {
                            let selected: Vec<SceneId> = {
                                let mut query = world.query_filtered::<&SceneId, With<Selected>>();
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
                                    value: Box::new(editor_scene::materials::MaterialRef(material))
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
    let Some(mut font) = search.map(Single::into_inner) else {
        return;
    };
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
    rig: Option<Res<crate::palette_preview::PreviewRig>>,
    registry: Res<AppTypeRegistry>,
    mut subject: ResMut<crate::palette_preview::PreviewSubject>,
    mut commands: Commands,
) {
    if !state.is_changed() && !items.is_changed() {
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
                        // Left accent bar carries selection; the fill stays a
                        // quiet tint (modern list treatment, not a slab).
                        border: UiRect::left(px(2.0)),
                        border_radius: BorderRadius::all(px(style::radius::S)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(if selected {
                        style::color::selection().with_alpha(0.35)
                    } else {
                        Color::NONE
                    }),
                    BorderColor::all(if selected {
                        style::color::accent()
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
                Node {
                    padding: UiRect::all(px(style::space::S)),
                    ..default()
                },
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
    // Live 3D preview for placeable things (v1 parity): kinds + prefabs.
    use crate::palette_preview::Subject;
    let preview_subject = match &selected_payload {
        Some(PalettePayload::Prefab(id)) => Some(Subject::Prefab(*id)),
        Some(PalettePayload::Action(id)) => id
            .as_str()
            .strip_prefix("insert.kind.")
            .map(|kind| Subject::Kind(EntityKindId::new(kind.to_string()))),
        _ => None,
    };
    if subject.0 != preview_subject {
        subject.0 = preview_subject.clone();
    }
    commands.entity(*preview).with_children(|pane| {
        if preview_subject.is_some() {
            pane.spawn((
                Text::new(selected_label.clone().unwrap_or_default()),
                style::sans_medium(&fonts, ui.font_size_m),
            ));
            if let Some(rig) = &rig {
                pane.spawn((
                    ImageNode::new(rig.image.clone()),
                    Node {
                        width: percent(100),
                        aspect_ratio: Some(1.0),
                        margin: UiRect::vertical(px(style::space::XS)),
                        ..default()
                    },
                ));
            }
            pane.spawn((
                Text::new("\u{23ce} place at cursor"),
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
            ));
            return;
        }
        if let Some(PalettePayload::AddComponent(type_id)) = &selected_payload {
            // Docs preview (owner + v1 lineage): name, full path, and the
            // component's doc comment straight from reflection.
            let registry = registry.read();
            if let Some(registration) = registry.get(*type_id) {
                let info = registration.type_info();
                let type_path = info.type_path();
                let short = info.type_path_table().short_path();
                // The pane announces its role — "two sections" read as a
                // mystery without it (owner).
                pane.spawn((
                    Text::new("DOCS"),
                    style::sans_medium(&fonts, ui.font_size_xs),
                    TextColor(style::color::TEXT_DIM),
                ));
                pane.spawn((
                    Text::new(short.to_string()),
                    style::sans_medium(&fonts, ui.font_size_m),
                ));
                pane.spawn((
                    Text::new(type_path.to_string()),
                    style::mono(&fonts, ui.font_size_xs),
                    TextColor(style::color::TEXT_DIM),
                ));
                // Doc comments arrive with raw line breaks + indent — flow
                // them into paragraphs so wrapping happens where the LAYOUT
                // needs it, never mid-sentence at source-code widths.
                let docs = info
                    .docs()
                    .unwrap_or("No documentation.")
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                pane.spawn((
                    Text::new(docs),
                    style::sans(&fonts, ui.font_size_s),
                    TextColor(style::color::TEXT_KEYS),
                    Node {
                        margin: UiRect::top(px(style::space::S)),
                        max_width: px(300.0),
                        ..default()
                    },
                ));
                let insertable = registration
                    .data::<bevy::reflect::std_traits::ReflectDefault>()
                    .is_some();
                pane.spawn((
                    Text::new(if insertable {
                        "\u{23ce} add to selection · \u{2303}f favorite"
                    } else {
                        "not insertable — no Default impl · \u{2303}f favorite"
                    }),
                    style::sans(&fonts, ui.font_size_s),
                    TextColor(style::color::TEXT_DIM),
                    Node {
                        margin: UiRect::top(px(style::space::S)),
                        ..default()
                    },
                ));
            }
            return;
        }
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
                Node {
                    margin: UiRect::top(px(style::space::XS)),
                    ..default()
                },
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
        pane.spawn((
            Text::new(def.name.to_string()),
            style::sans_medium(&fonts, ui.font_size_m),
        ));
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
                Node {
                    margin: UiRect::top(px(style::space::XS)),
                    ..default()
                },
            ));
        }
        let bindings: Vec<String> = keymap
            .by_context
            .iter()
            .flat_map(|(context, entries)| {
                entries
                    .iter()
                    .filter(|(_, action)| action == &def.id)
                    .map(move |(binding, _)| {
                        format!("{}  ·  {}", style::pretty_binding(binding), context)
                    })
            })
            .collect();
        if !bindings.is_empty() {
            pane.spawn((
                Text::new("BINDINGS"),
                style::sans(&fonts, ui.font_size_xs),
                TextColor(style::color::TEXT_DIM),
                Node {
                    margin: UiRect::top(px(style::space::S)),
                    ..default()
                },
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

/// The ground point at the center of the viewport camera's gaze — the "somewhere
/// visible" placement fallback when the cursor isn't over the viewport.
fn camera_focus_ground(world: &mut World) -> Option<Vec3> {
    let mut query = world.query::<(
        &bevy::camera::Camera,
        &GlobalTransform,
        Option<&bevy::camera::RenderTarget>,
    )>();
    let (_, transform, _) = query
        .iter(world)
        .find(|(c, _, target)| is_viewport_camera(c, *target))?;
    let ray = bevy::math::Ray3d::new(transform.translation(), transform.forward());
    // Gaze-ground intersection when the camera looks at the floor; a level (or
    // upward) camera never intersects, so drop a point a few meters ahead onto
    // the ground instead — always on-screen, never at a far-away grazing hit.
    let hit = ray
        .intersect_plane(
            Vec3::ZERO,
            bevy::math::primitives::InfinitePlane3d::new(Vec3::Y),
        )
        .filter(|d| *d < 50.0)
        .map(|d| ray.get_point(d));
    Some(hit.unwrap_or_else(|| {
        let ahead = transform.translation() + *transform.forward() * 6.0;
        Vec3::new(ahead.x, 0.0, ahead.z)
    }))
}
