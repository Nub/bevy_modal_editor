//! Dock shell (RFC §9): renders the kernel's `PanelCatalog` into left/right/bottom
//! docks with UNIFORM chrome — a panel can never draw its own window chrome. Bodies
//! are filled by content renderers (hierarchy, properties) that target `PanelBody`;
//! this module owns docking, headers, focus visuals, and click-to-focus.

use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui::px;
use editor_core::prelude::*;

use crate::style::{self, UiFonts};

#[derive(Component)]
pub(crate) struct DockRoot(#[allow(dead_code)] Placement);

#[derive(Component)]
pub(crate) struct PanelCard(pub PanelId);

#[derive(Component)]
pub(crate) struct PanelHeader(PanelId);

/// The content slot for a panel — hierarchy/properties renderers parent into this.
#[derive(Component)]
pub(crate) struct PanelBody(#[allow(dead_code, reason = "read by content renderers (C2/C3)")] pub PanelId);

/// Placeholder shown until a content renderer fills the body.
#[derive(Component)]
struct PanelEmptyState;

pub(crate) fn spawn_docks(
    mut commands: Commands,
    catalog: Res<PanelCatalog>,
    settings: Res<EditorSettings>,
    fonts: Res<UiFonts>,
) {
    let ui = settings.ui.clone();
    for placement in [Placement::Left, Placement::Right, Placement::Bottom] {
        let panels: Vec<_> = catalog.in_placement(placement).cloned().collect();
        if panels.is_empty() {
            continue;
        }
        let mut node = Node {
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            row_gap: px(style::space::S),
            padding: UiRect::all(px(style::space::S)),
            ..default()
        };
        match placement {
            Placement::Left => {
                node.left = px(0);
                node.top = px(0);
                node.bottom = px(style::BAR_HEIGHT);
                node.width = px(ui.dock_left_width);
            }
            Placement::Right => {
                node.right = px(0);
                node.top = px(0);
                node.bottom = px(style::BAR_HEIGHT);
                node.width = px(ui.dock_right_width);
            }
            Placement::Bottom => {
                node.left = px(ui.dock_left_width);
                node.right = px(ui.dock_right_width);
                node.bottom = px(style::BAR_HEIGHT);
                node.height = px(ui.dock_bottom_height);
            }
        }
        commands
            .spawn((
                DockRoot(placement),
                node,
                GlobalZIndex(50),
                Visibility::Hidden,
            ))
            .with_children(|dock| {
                for decl in &panels {
                    let id = decl.id.clone();
                    let mut card = dock.spawn((
                        PanelCard(id.clone()),
                        Node {
                            flex_direction: FlexDirection::Column,
                            flex_grow: 1.0,
                            flex_basis: px(0),
                            min_height: px(0),
                            border: UiRect::all(px(1.0)),
                            border_radius: BorderRadius::all(px(style::radius::L)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        ThemeBackgroundColor(tokens::WINDOW_BG),
                        BorderColor::all(style::HAIRLINE),
                    ));
                    card.observe(
                        move |press: On<Pointer<Press>>,
                              mut focus: ResMut<PanelFocus>,
                              cards: Query<&PanelCard>| {
                            // Any click inside the card focuses its panel (bubbled
                            // presses re-target ancestors; resolve via the card itself).
                            if let Ok(card) = cards.get(press.entity) {
                                focus.0 = Some(card.0.clone());
                            }
                        },
                    );
                    card.with_children(|card| {
                        card.spawn((
                            PanelHeader(id.clone()),
                            Node {
                                padding: UiRect::axes(px(style::space::S), px(style::space::XS)),
                                align_items: AlignItems::Center,
                                column_gap: px(style::space::S),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            ThemeBackgroundColor(tokens::PANE_HEADER_BG),
                        ))
                        .with_children(|header| {
                            header.spawn((
                                Text::new(decl.title.to_uppercase()),
                                style::sans_medium(&fonts, ui.font_size_xs),
                                TextColor(style::color::TEXT_DIM),
                            ));
                        });
                        card.spawn((
                            PanelBody(id.clone()),
                            Node {
                                flex_direction: FlexDirection::Column,
                                flex_grow: 1.0,
                                min_height: px(0),
                                padding: UiRect::all(px(style::space::S)),
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                        ))
                        .with_children(|body| {
                            body.spawn((
                                PanelEmptyState,
                                Text::new("empty"),
                                style::sans(&fonts, ui.font_size_s),
                                TextColor(style::color::TEXT_DIM),
                            ));
                        });
                    });
                }
            });
    }
}

/// Docks show while the editor owns input; individual cards follow `PanelStates`;
/// the focused card gets the accent border + title (the ONE focus treatment).
pub(crate) fn sync_dock_chrome(
    state: Res<EditorState>,
    states: Res<PanelStates>,
    focus: Res<PanelFocus>,
    mut docks: Query<(&DockRoot, &mut Visibility), Without<PanelCard>>,
    mut cards: Query<(&PanelCard, &mut Visibility, &mut BorderColor), Without<DockRoot>>,
    mut headers: Query<(&PanelHeader, &Children)>,
    mut titles: Query<&mut TextColor>,
) {
    for (dock, mut visibility) in &mut docks {
        let any_open = states.0.iter().any(|(id, open)| {
            *open && cards.iter().any(|(card, _, _)| &card.0 == id && dock_has(dock, card))
        });
        *visibility = if state.active && any_open { Visibility::Visible } else { Visibility::Hidden };
    }
    for (card, mut visibility, mut border) in &mut cards {
        *visibility =
            if states.open(&card.0) { Visibility::Inherited } else { Visibility::Hidden };
        let focused = focus.0.as_ref() == Some(&card.0);
        let target = if focused { style::color::accent() } else { style::HAIRLINE };
        if border.top != target {
            *border = BorderColor::all(target);
        }
    }
    for (header, children) in &mut headers {
        let focused = focus.0.as_ref() == Some(&header.0);
        let target = if focused { style::color::accent() } else { style::color::TEXT_DIM };
        for child in children {
            if let Ok(mut color) = titles.get_mut(*child) {
                if color.0 != target {
                    color.0 = target;
                }
            }
        }
    }
}

fn dock_has(_dock: &DockRoot, _card: &PanelCard) -> bool {
    // Cards live inside their dock's hierarchy; per-dock membership is structural.
    true
}

/// Writes the kernel's `PointerOverChrome` gate: cursor inside any visible UI node
/// (docks, statusbar, palette, popups) means viewport tools must stand down.
pub(crate) fn track_pointer_over_chrome(
    windows: Query<&Window, With<bevy::window::PrimaryWindow>>,
    nodes: Query<(&ComputedNode, &UiGlobalTransform, &InheritedVisibility)>,
    mut blocked: ResMut<PointerOverChrome>,
) {
    let over = windows
        .single()
        .ok()
        .and_then(|window| {
            let cursor = window.cursor_position()?;
            let physical = cursor * window.scale_factor();
            Some(nodes.iter().any(|(node, transform, visibility)| {
                if !visibility.get() || node.size() == Vec2::ZERO {
                    return false;
                }
                let half = node.size() / 2.0;
                let center = transform.translation;
                (physical.x - center.x).abs() <= half.x && (physical.y - center.y).abs() <= half.y
            }))
        })
        .unwrap_or(false);
    if blocked.0 != over {
        blocked.0 = over;
    }
}
