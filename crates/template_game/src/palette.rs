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

use crate::ui_style::{self as style, UiFont};

const MAX_RESULTS: usize = 8;

#[derive(Resource, Default)]
pub struct PaletteState {
    pub open: bool,
    pub query: String,
    pub selected: usize,
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

pub struct PalettePlugin;

impl Plugin for PalettePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PaletteState>()
            .add_systems(Startup, spawn_palette)
            .add_systems(
                Update,
                (handle_open_action, close_when_editor_leaves, rebuild_results)
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
            border_radius: {BorderRadius::all(px(style::radius::L))},
        }
        ThemeBackgroundColor(tokens::WINDOW_BG)
        GlobalZIndex(200)
        Visibility::Hidden
        Children [
            // Mode title (v1 lineage): what this palette is browsing, uppercase, dim.
            (PaletteTitle Text("COMMANDS")
             template(|_ctx| Ok(bevy::text::TextFont {
                 font_size: bevy::text::FontSize::Px(11.0),
                 ..Default::default()
             }))
             TextColor({crate::ui_style::color::TEXT_DIM})),
            // Search row: mode badge glyph + input.
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(style::space::S),
                    align_items: AlignItems::Center,
                }
                Children [
                    (
                        Node {
                            width: px(26),
                            height: px(24),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: {BorderRadius::all(px(style::radius::S))},
                        }
                        BackgroundColor({style::color::accent()})
                        Children [
                            (
                                Text({style::glyph::SEARCH.to_string()})
                                template(|ctx| Ok(bevy::text::TextFont {
                                    font: bevy::text::FontSource::Handle(
                                        ctx.resource::<AssetServer>()
                                            .load("fonts/FiraCodeNerdFont-Regular.ttf"),
                                    ),
                                    font_size: bevy::text::FontSize::Px(13.0),
                                    ..Default::default()
                                }))
                                TextColor({style::color::TEXT_ON_ACCENT})
                            )
                        ]
                    ),
                    (
                        @FeathersTextInputContainer
                        // Container defaults to centering its child; with a fixed-width
                        // input that reads as a random left gap — pin to the start edge.
                        Node { flex_grow: 1.0, justify_content: JustifyContent::FlexStart }
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

/// Case-insensitive substring filter over id + name; hidden actions excluded.
/// Returns (label, binding-hint, action id).
fn filter_actions(
    catalog: &ActionCatalog,
    keymap: &ResolvedKeymap,
    query: &str,
) -> Vec<(String, String, ActionId)> {
    let needle = query.to_lowercase();
    let mut out = Vec::new();
    for def in &catalog.actions {
        if def.flags.hidden {
            continue;
        }
        let hay = format!("{} {}", def.id.as_str(), def.name).to_lowercase();
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
        if out.len() >= MAX_RESULTS {
            break;
        }
    }
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
    capture.0 = false;
    focus.clear();
    *root_vis = Visibility::Hidden;
}

fn handle_open_action(
    mut reader: MessageReader<ActionInvoked>,
    mut state: ResMut<PaletteState>,
    mut capture: ResMut<KeyCapture>,
    mut focus: ResMut<InputFocus>,
    input: Single<Entity, With<PaletteInput>>,
    mut root: Single<&mut Visibility, With<PaletteRoot>>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() == "core.palette" && !state.open {
            open_palette(&mut state, &mut capture, &mut focus, *input, &mut root);
        }
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
    let result_count = filter_actions(&catalog, &keymap, &state.query).len();
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
            let results = filter_actions(&catalog, &keymap, &state.query);
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

fn rebuild_results(
    state: Res<PaletteState>,
    catalog: Res<ActionCatalog>,
    keymap: Res<ResolvedKeymap>,
    results: Single<Entity, With<PaletteResults>>,
    preview: Single<Entity, With<PalettePreview>>,
    font: Res<UiFont>,
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
    let rows = filter_actions(&catalog, &keymap, &state.query);

    // Left pane: the result list.
    commands.entity(*results).with_children(|parent| {
        for (i, (label, binding, _)) in rows.iter().enumerate() {
            let selected = i == state.selected;
            parent
                .spawn((
                    Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(px(style::space::S), px(style::space::XS)),
                        column_gap: px(style::space::M),
                        border_radius: BorderRadius::all(px(style::radius::S)),
                        ..default()
                    },
                    BackgroundColor(if selected { style::color::selection() } else { Color::NONE }),
                ))
                .with_children(|row| {
                    row.spawn((
                        Text::new(label.clone()),
                        style::sans(style::font_size::M),
                    ));
                    if !binding.is_empty() {
                        row.spawn((
                            Text::new(binding.clone()),
                            style::mono(&font, style::font_size::S),
                            TextColor(style::color::TEXT_KEYS),
                        ));
                    }
                });
        }
        if rows.is_empty() {
            parent.spawn((
                Text::new("no matching actions"),
                style::sans(style::font_size::S),
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
                style::sans(style::font_size::S),
                TextColor(style::color::TEXT_DIM),
            ));
            return;
        };
        pane.spawn((Text::new(def.name.to_string()), style::sans(style::font_size::M)));
        pane.spawn((
            Text::new(def.id.to_string()),
            style::mono(&font, style::font_size::XS),
            TextColor(style::color::TEXT_DIM),
        ));
        if !def.description.is_empty() {
            pane.spawn((
                Text::new(def.description.to_string()),
                style::sans(style::font_size::S),
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
                style::sans(style::font_size::XS),
                TextColor(style::color::TEXT_DIM),
                Node { margin: UiRect::top(px(style::space::S)), ..default() },
            ));
            for line in bindings {
                pane.spawn((
                    Text::new(line),
                    style::mono(&font, style::font_size::S),
                    TextColor(style::color::TEXT_KEYS),
                ));
            }
        }
    });
}
