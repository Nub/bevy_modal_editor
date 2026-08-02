//! Command palette (M1 gate: "open the palette, fuzzy-find an action, invoke it").
//!
//! Lists the `ActionCatalog` (derived — never hand-maintained) with current bindings,
//! filters as you type, Up/Down/Enter/Esc navigation. Applies the M0 spike-3 lessons:
//! `visible_width` on the input (F7), `SelectAllOnFocus`, whole-box focus, and
//! `KeyCapture` so the resolver stands down while typing.
//!
//! M1-minimal: substring match, top 8 rows, plain row rendering. The real fuzzy
//! engine and widget-kit treatment land in `editor_ui`.

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

use crate::ui_style::{self as style, UiFonts};

const MAX_RESULTS: usize = 50;

/// What this palette is browsing (v1's typed open-modes, spec §7): filters are
/// structural, never query-string hacks.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum PaletteFilter {
    #[default]
    Commands,
    InsertKinds,
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
/// Marks the highlighted result row (scroll-follow target).
#[derive(Component)]
struct SelectedRow;

pub struct PalettePlugin;

impl Plugin for PalettePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PaletteState>()
            .add_systems(Startup, spawn_palette)
            .add_systems(
                Update,
                (
                    handle_open_action,
                    open_on_insert_mode,
                    close_when_editor_leaves,
                    close_when_focus_leaves,
                    update_title,
                    rebuild_results,
                    scroll_selected_into_view,
                    probe_scroll.run_if(|| std::env::var("PALETTE_PROBE").is_ok()),
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
        BorderColor::all({style::HAIRLINE})
        template_value({style::floating_shadow()})
        GlobalZIndex(200)
        Visibility::Hidden
        Children [
            // Mode title (v1 lineage): what this palette is browsing, uppercase, dim.
            (PaletteTitle Text("COMMANDS")
             template(|ctx| Ok(bevy::text::TextFont {
                 font: bevy::text::FontSource::Handle(
                     ctx.resource::<AssetServer>().load(crate::ui_style::SANS_MEDIUM_PATH),
                 ),
                 font_size: bevy::text::FontSize::Px(11.0),
                 ..Default::default()
             }))
             TextColor({crate::ui_style::color::TEXT_DIM})),
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

/// Case-insensitive substring filter over id + name + description ("load" must find
/// "Open Scene" via its describe text); hidden actions excluded; sorted by name so
/// the list is deterministic, not registration-ordered.
/// Returns (label, binding-hint, action id).
fn filter_actions(
    catalog: &ActionCatalog,
    keymap: &ResolvedKeymap,
    query: &str,
    filter: PaletteFilter,
) -> Vec<(String, String, ActionId)> {
    let needle = query.to_lowercase();
    let mut out = Vec::new();
    for def in &catalog.actions {
        if def.flags.hidden {
            continue;
        }
        let is_kind = def.id.as_str().starts_with("insert.kind.");
        match filter {
            PaletteFilter::InsertKinds if !is_kind => continue,
            _ => {}
        }
        let hay =
            format!("{} {} {}", def.id.as_str(), def.name, def.description).to_lowercase();
        if !needle.is_empty() && !hay.contains(&needle) {
            continue;
        }
        let binding = keymap
            .by_context
            .values()
            .flatten()
            .find(|(_, action)| action == &def.id)
            .map(|(binding, _)| style::pretty_binding(binding))
            .unwrap_or_default();
        out.push((def.name.to_string(), binding, def.id.clone()));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out.truncate(MAX_RESULTS);
    out
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
    catalog: Res<ActionCatalog>,
    keymap: Res<ResolvedKeymap>,
    mut state: ResMut<PaletteState>,
    mut capture: ResMut<KeyCapture>,
    mut focus: ResMut<InputFocus>,
    mut root: Single<&mut Visibility, With<PaletteRoot>>,
    mut actions: MessageWriter<ActionInvoked>,
) {
    if !state.open || event.input.state != ButtonState::Pressed {
        return;
    }
    let result_count = filter_actions(&catalog, &keymap, &state.query, state.filter).len();
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
            let results = filter_actions(&catalog, &keymap, &state.query, state.filter);
            if let Some((_, _, action)) = results.get(state.selected) {
                actions.write(ActionInvoked {
                    action: action.clone(),
                    args: None,
                    source: InvocationSource::Palette,
                });
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

/// TEMP diagnostic (PALETTE_PROBE=1): auto-open the palette, walk the selection
/// down, and log the real laid-out geometry every step.
fn probe_scroll(
    mut frames: Local<u32>,
    mut writer: MessageWriter<ActionInvoked>,
    mut state: ResMut<PaletteState>,
    container: Option<
        Single<(&Node, &ComputedNode, &UiGlobalTransform, &ScrollPosition), With<PaletteResults>>,
    >,
    row: Option<
        Single<(&ComputedNode, &UiGlobalTransform), (With<SelectedRow>, Without<PaletteResults>)>,
    >,
) {
    *frames += 1;
    if *frames == 60 {
        writer.write(ActionInvoked {
            action: ActionId::new("core.toggle-editor".to_string()),
            args: None,
            source: InvocationSource::Test,
        });
    }
    if *frames == 120 {
        writer.write(ActionInvoked {
            action: ActionId::new("core.palette".to_string()),
            args: None,
            source: InvocationSource::Test,
        });
    }
    if *frames > 180 && *frames % 30 == 0 && state.open {
        state.selected += 1;
        if let Some(container) = &container {
            let (node, cn, tf, scroll) = &**container;
            info!(
                "PROBE sel={} overflow={:?} cont_size={:?} content={:?} scroll={:?} tf={:?}",
                state.selected,
                node.overflow,
                cn.size(),
                cn.content_size,
                scroll.0,
                tf.translation,
            );
        } else {
            info!("PROBE: no container");
        }
        match &row {
            Some(row) => {
                let (cn, tf) = &**row;
                info!("PROBE row size={:?} tf={:?}", cn.size(), tf.translation);
            }
            None => info!("PROBE: no SelectedRow"),
        }
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

fn rebuild_results(
    state: Res<PaletteState>,
    catalog: Res<ActionCatalog>,
    keymap: Res<ResolvedKeymap>,
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
    let rows = filter_actions(&catalog, &keymap, &state.query, state.filter);

    // Left pane: the result list.
    commands.entity(*results).with_children(|parent| {
        for (i, (label, binding, _)) in rows.iter().enumerate() {
            let selected = i == state.selected;
            let mut row = parent
                .spawn((
                    Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(px(style::space::S), px(style::space::XS)),
                        column_gap: px(style::space::M),
                        border_radius: BorderRadius::all(px(style::radius::S)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BackgroundColor(if selected { style::color::selection() } else { Color::NONE }),
                ));
            if selected {
                row.insert(SelectedRow);
            }
            row.with_children(|row| {
                    row.spawn((
                        Text::new(label.clone()),
                        style::sans(&fonts, style::font_size::M),
                    ));
                    if !binding.is_empty() {
                        row.spawn((
                            Text::new(binding.clone()),
                            style::mono(&fonts, style::font_size::S),
                            TextColor(style::color::TEXT_KEYS),
                        ));
                    }
                });
        }
        if rows.is_empty() {
            parent.spawn((
                Text::new("no matching actions"),
                style::sans(&fonts, style::font_size::S),
                TextColor(style::color::TEXT_DIM),
                Node { padding: UiRect::all(px(style::space::S)), ..default() },
            ));
        }
    });

    // Right pane: preview/docs for the selection. Actions have no visual preview, so
    // this shows documentation — the same surface previews assets/prefabs later.
    let selected_def = rows
        .get(state.selected)
        .and_then(|(_, _, id)| catalog.get(id).cloned());
    commands.entity(*preview).with_children(|pane| {
        let Some(def) = selected_def else {
            pane.spawn((
                Text::new("no selection"),
                style::sans(&fonts, style::font_size::S),
                TextColor(style::color::TEXT_DIM),
            ));
            return;
        };
        pane.spawn((Text::new(def.name.to_string()), style::sans_medium(&fonts, style::font_size::M)));
        pane.spawn((
            Text::new(def.id.to_string()),
            style::mono(&fonts, style::font_size::XS),
            TextColor(style::color::TEXT_DIM),
        ));
        if !def.description.is_empty() {
            pane.spawn((
                Text::new(def.description.to_string()),
                style::sans(&fonts, style::font_size::S),
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
                style::sans(&fonts, style::font_size::XS),
                TextColor(style::color::TEXT_DIM),
                Node { margin: UiRect::top(px(style::space::S)), ..default() },
            ));
            for line in bindings {
                pane.spawn((
                    Text::new(line),
                    style::mono(&fonts, style::font_size::S),
                    TextColor(style::color::TEXT_KEYS),
                ));
            }
        }
    });
}
