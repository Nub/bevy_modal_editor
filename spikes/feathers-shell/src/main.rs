//! M0 Spike 3 — feathers-shell (spikes/README.md).
//!
//! Can bevy_ui + feathers (Bevy 0.19) carry the editor shell? Known gaps from source
//! review: NO list virtualization and NO docking/splitters exist upstream — so this
//! spike hand-rolls both on feathers' building blocks, plus exercises what feathers
//! does provide: property-grid controls (number inputs with axis tints, slider,
//! checkbox), text input (parley: selection/clipboard/IME), theming, tab focus.
//!
//! Layout: [hierarchy pane: 10k-row VIRTUAL list] | draggable splitter | [inspector
//! pane: property grid] with a status bar showing value flow.
//!
//! Run: cargo run -p spike_feathers_shell --release
//! Judge interactively: scroll the big list (entity count stays ~constant — printed),
//! drag the splitter, tab through controls, type/select/copy in the text field.

use bevy::feathers::{
    controls::{
        FeathersCheckbox, FeathersNumberInput, FeathersSlider, FeathersTextInput,
        FeathersTextInputContainer,
    },
    dark_theme::create_dark_theme,
    theme::{ThemeBackgroundColor, ThemedText, UiTheme},
    tokens, FeathersPlugins,
};
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::scene::SpawnListSystem;
use bevy::text::TextEditChange;
use bevy::ui::{px, percent, Checked, ScrollPosition};
use bevy::ui_widgets::{
    checkbox_self_update, slider_self_update, ScrollArea, SliderPrecision, SliderStep, ValueChange,
};
use bevy::window::SystemCursorIcon;
use bevy::feathers::cursor::EntityCursor;

const TOTAL_ROWS: usize = 10_000;
const ROW_HEIGHT: f32 = 24.0;
const VISIBLE_ROWS: usize = 48; // viewport rows + overscan

#[derive(Resource)]
struct ListData {
    items: Vec<String>,
}

#[derive(Resource, Default)]
struct DemoState {
    position: Vec3,
    scale: f32,
    visible: bool,
    name: String,
}

#[derive(Component, Default, Clone)]
struct VirtualScroll;
#[derive(Component, Default, Clone)]
struct TopSpacer;
#[derive(Component, Default, Clone)]
struct BottomSpacer;
#[derive(Component, Default, Clone)]
struct VirtualRow(usize); // slot index 0..VISIBLE_ROWS
#[derive(Component, Default, Clone)]
struct LeftPane;
#[derive(Component, Default, Clone)]
struct StatusText;
#[derive(Component, Default, Clone)]
struct NameInput;

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, FeathersPlugins))
        .insert_resource(UiTheme(create_dark_theme()))
        .insert_resource(ListData {
            items: (0..TOTAL_ROWS).map(|i| format!("Entity {i:05}  (Cube)")).collect(),
        })
        .init_resource::<DemoState>()
        .add_systems(Startup, shell.spawn())
        .add_systems(Update, (virtualize_list, update_status, report_entity_count))
        .run();
}

fn shell() -> impl SceneList {
    bsn_list![Camera2d, root()]
}

fn root() -> impl Scene {
    let rows: Vec<_> = (0..VISIBLE_ROWS).map(row_slot).collect();
    bsn! {
        Node {
            width: percent(100),
            height: percent(100),
            flex_direction: FlexDirection::Column,
        }
        TabGroup
        ThemeBackgroundColor(tokens::WINDOW_BG)
        Children [
            // main area: left pane | splitter | right pane
            (
                Node { flex_grow: 1.0, flex_direction: FlexDirection::Row, min_height: px(0) }
                Children [
                    // ---- hierarchy pane: hand-rolled VIRTUAL list over 10k rows ----
                    (
                        LeftPane
                        Node {
                            width: px(320),
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                        }
                        ThemeBackgroundColor(tokens::PANE_BODY_BG)
                        Children [
                            (Text("HIERARCHY — 10,000 rows, virtualized") ThemedText
                             Node { padding: UiRect::all(px(6)), flex_shrink: 0.0 }),
                            (
                                VirtualScroll
                                Node {
                                    flex_grow: 1.0,
                                    min_height: px(0),
                                    overflow: Overflow::scroll_y(),
                                    flex_direction: FlexDirection::Column,
                                }
                                ScrollArea
                                Children [
                                    (TopSpacer Node { height: px(0), flex_shrink: 0.0 }),
                                    {rows},
                                    (BottomSpacer Node {
                                        height: px((TOTAL_ROWS - VISIBLE_ROWS) as f32 * ROW_HEIGHT),
                                        flex_shrink: 0.0,
                                    }),
                                ]
                            ),
                        ]
                    ),
                    // ---- hand-rolled draggable splitter (upstream has none) ----
                    (
                        Node { width: px(5), flex_shrink: 0.0 }
                        ThemeBackgroundColor(tokens::PANE_HEADER_DIVIDER)
                        EntityCursor::System(SystemCursorIcon::EwResize)
                        on(|drag: On<Pointer<Drag>>, mut pane: Single<&mut Node, With<LeftPane>>| {
                            if let Val::Px(w) = pane.width {
                                pane.width = px((w + drag.delta.x).clamp(160.0, 640.0));
                            }
                        })
                    ),
                    // ---- inspector pane: property grid from feathers controls ----
                    inspector_pane(),
                ]
            ),
            // status bar: proves value flow from widgets to state
            (
                StatusText
                Text("status") ThemedText
                Node { padding: UiRect::all(px(6)), flex_shrink: 0.0 }
                ThemeBackgroundColor(tokens::PANE_HEADER_BG)
            ),
        ]
    }
}

fn row_slot(slot: usize) -> impl Scene {
    bsn! {
        VirtualRow({slot})
        Text({format!("row {slot}")})
        ThemedText
        Node { height: px(ROW_HEIGHT), flex_shrink: 0.0, padding: UiRect::horizontal(px(8)) }
        on(|click: On<Pointer<Click>>, rows: Query<&VirtualRow>| {
            if let Ok(row) = rows.get(click.entity) {
                info!("clicked virtual slot {}", row.0);
            }
        })
    }
}

fn axis_input(label: &'static str, sigil: bevy::feathers::theme::ThemeToken) -> impl Scene {
    bsn! {
        @FeathersNumberInput {
            @sigil_color: {sigil},
            @label_text: {Some(label)},
        }
        Node { flex_grow: 1.0, max_width: px(110) }
    }
}

fn inspector_pane() -> impl Scene {
    bsn! {
        Node {
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(8)),
            row_gap: px(8),
        }
        ThemeBackgroundColor(tokens::PANE_BODY_BG)
        Children [
            (Text("INSPECTOR") ThemedText),
            // Gallery idiom: labels on their OWN line above each control row — inline
            // labels overflow because EditableText inputs have large intrinsic
            // min-widths (this was the overlap bug).
            // Name: feathers text input (parley editing: selection/clipboard/IME)
            (Text("Name") ThemedText),
            (
                @FeathersTextInputContainer
                Children [
                    (
                        @FeathersTextInput {}
                        NameInput
                        on(|_c: On<TextEditChange>,
                            q: Single<&bevy::text::EditableText, With<NameInput>>,
                            mut state: ResMut<DemoState>| {
                            state.name = q.value().to_string();
                        })
                    )
                ]
            ),
            // Position: three axis-tinted number inputs (the property-grid primitive)
            (Text("Position") ThemedText),
            (
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: px(6),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                }
                Children [
                    (
                        axis_input("X", tokens::TEXT_INPUT_X_AXIS)
                        on(|vc: On<ValueChange<f32>>, mut s: ResMut<DemoState>| { s.position.x = vc.value; })
                    ),
                    (
                        axis_input("Y", tokens::TEXT_INPUT_Y_AXIS)
                        on(|vc: On<ValueChange<f32>>, mut s: ResMut<DemoState>| { s.position.y = vc.value; })
                    ),
                    (
                        axis_input("Z", tokens::TEXT_INPUT_Z_AXIS)
                        on(|vc: On<ValueChange<f32>>, mut s: ResMut<DemoState>| { s.position.z = vc.value; })
                    ),
                ]
            ),
            // Scale: slider (gallery-exact: SliderStep + SliderPrecision, no wrapping row)
            (Text("Scale") ThemedText),
            (
                @FeathersSlider { @max: 10.0, @value: 1.0 }
                SliderStep(0.5)
                SliderPrecision(2)
                on(slider_self_update)
                on(|vc: On<ValueChange<f32>>, mut s: ResMut<DemoState>| { s.scale = vc.value; })
            ),
            // Visible: checkbox
            (
                @FeathersCheckbox { @caption: bsn! { Text("Visible") ThemedText } }
                Checked
                on(checkbox_self_update)
                on(|vc: On<ValueChange<bool>>, mut s: ResMut<DemoState>| { s.visible = vc.value; })
            ),
        ]
    }
}

/// The hand-rolled virtualization: on scroll, re-bind the fixed slot entities to the
/// data window and resize the spacers. Slot entities never grow with data size.
fn virtualize_list(
    data: Res<ListData>,
    scroll: Query<&ScrollPosition, (With<VirtualScroll>, Changed<ScrollPosition>)>,
    mut top: Query<&mut Node, (With<TopSpacer>, Without<BottomSpacer>, Without<VirtualRow>)>,
    mut bottom: Query<&mut Node, (With<BottomSpacer>, Without<TopSpacer>, Without<VirtualRow>)>,
    mut rows: Query<(&VirtualRow, &mut Text)>,
) {
    let Ok(pos) = scroll.single() else { return };
    let first = ((pos.y / ROW_HEIGHT) as usize).min(data.items.len().saturating_sub(VISIBLE_ROWS));
    for mut node in &mut top {
        node.height = px(first as f32 * ROW_HEIGHT);
    }
    for mut node in &mut bottom {
        node.height = px((data.items.len() - first - VISIBLE_ROWS) as f32 * ROW_HEIGHT);
    }
    for (slot, mut text) in &mut rows {
        let idx = first + slot.0;
        if text.0 != data.items[idx] {
            text.0 = data.items[idx].clone();
        }
    }
}

fn update_status(state: Res<DemoState>, mut q: Query<&mut Text, With<StatusText>>) {
    if !state.is_changed() {
        return;
    }
    for mut t in &mut q {
        t.0 = format!(
            "name: {:?}   pos: [{:.1}, {:.1}, {:.1}]   scale: {:.2}   visible: {}",
            state.name, state.position.x, state.position.y, state.position.z, state.scale, state.visible
        );
    }
}

/// Prove virtualization: total UI entity count must stay ~constant regardless of the
/// 10k data rows. Printed once at startup and every 5s.
fn report_entity_count(nodes: Query<(), With<Node>>, time: Res<Time>, mut last: Local<f32>) {
    if *last == 0.0 || time.elapsed_secs() - *last > 5.0 {
        *last = time.elapsed_secs();
        info!("UI node entities: {} (10,000 data rows)", nodes.iter().count());
    }
}
