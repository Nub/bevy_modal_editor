//! Dock shell (RFC §9): renders the kernel's `PanelCatalog` into left/right/bottom
//! docks with UNIFORM chrome — a panel can never draw its own window chrome. Bodies
//! are filled by content renderers (hierarchy, properties) that target `PanelBody`;
//! this module owns docking, headers, focus visuals, and click-to-focus.

use bevy::feathers::controls::FeathersScrollbar;
use bevy::feathers::theme::ThemeBackgroundColor;
use bevy::feathers::tokens;
use bevy::prelude::*;
use bevy::ui::px;
use bevy::ui_widgets::ControlOrientation;
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
pub(crate) struct PanelBody(pub PanelId);

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
                                // Titles read LARGER than panel contents and sit
                                // comfortably in the frame (owner).
                                padding: UiRect::axes(px(style::space::M), px(style::space::S)),
                                align_items: AlignItems::Center,
                                column_gap: px(style::space::S),
                                flex_shrink: 0.0,
                                // Round the header's own top corners: the card must
                                // never square-clip its border radius (owner call —
                                // clipped corners are a review rejection).
                                border_radius: BorderRadius::top(px(style::radius::L - 1.0)),
                                // Fixed header, scrolling content: a hairline
                                // delineates the boundary (owner).
                                border: UiRect::bottom(px(1.0)),
                                ..default()
                            },
                            ThemeBackgroundColor(tokens::PANE_HEADER_BG),
                            BorderColor::all(style::HAIRLINE),
                        ))
                        .with_children(|header| {
                            header.spawn((
                                Text::new(decl.title.to_string()),
                                style::sans_medium(&fonts, ui.font_size_m),
                                TextColor(style::color::TEXT_KEYS),
                            ));
                        });
                        card.spawn(Node {
                            flex_direction: FlexDirection::Column,
                            flex_grow: 1.0,
                            min_height: px(0),
                            ..default()
                        })
                        .insert(BodyWrapper)
                        .with_children(|wrapper| {
                        wrapper.spawn((
                            PanelBody(id.clone()),
                            bevy::input_focus::tab_navigation::TabGroup::default(),
                            // Wheel scrolling for every panel body.
                            bevy::ui_widgets::ScrollArea,
                            Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(style::space::XS),
                                flex_grow: 1.0,
                                min_height: px(0),
                                // Extra right padding: content never runs under
                                // the scrollbar overlay (bar + inset + breathing).
                                padding: UiRect {
                                    left: px(style::space::S),
                                    right: px(style::space::M + style::space::XS),
                                    top: px(style::space::S),
                                    bottom: px(style::space::S),
                                },
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
                    });
                }
            });
    }
}

/// Marks cards whose scrollbar overlay exists.
#[derive(Component)]
pub(crate) struct ScrollbarAttached;

/// The relative container spanning exactly the body region.
#[derive(Component)]
pub(crate) struct BodyWrapper;

/// Our scrollbar roots: animated width (macOS-style shrink when idle — an
/// indicator, not a focus).
#[derive(Component)]
pub(crate) struct EditorScrollbar {
    width: f32,
    last_active: f32,
}

/// Startup pass after `spawn_docks`: every panel body gets a feathers scrollbar
/// overlay (kit-first) pinned to its card's right edge, targeting the body's
/// `ScrollPosition`.
pub(crate) fn attach_scrollbars(
    cards: Query<(Entity, &Children), (With<PanelCard>, Without<ScrollbarAttached>)>,
    wrappers: Query<(Entity, &Children), With<BodyWrapper>>,
    bodies: Query<Entity, With<PanelBody>>,
    mut commands: Commands,
) {
    for (card, children) in &cards {
        let Some((wrapper, body)) = children.iter().find_map(|child| {
            let (wrapper, wrapper_children) = wrappers.get(child).ok()?;
            let body = wrapper_children.iter().find(|c| bodies.get(*c).is_ok())?;
            Some((wrapper, body))
        }) else {
            continue;
        };
        let scrollbar = commands
            .spawn_scene(bsn! {
                @FeathersScrollbar {
                    @target: {bevy::ecs::template::EntityTemplate::Entity(body)},
                    @orientation: {ControlOrientation::Vertical},
                }
                Node {
                    position_type: PositionType::Absolute,
                    right: px(style::space::XS),
                    top: px(style::space::XS),
                    bottom: px(style::space::XS),
                    width: px(3),
                }
            })
            .id();
        commands
            .entity(scrollbar)
            .insert((EditorScrollbar { width: 3.0, last_active: -10.0 }, ChildOf(wrapper)));
        commands.entity(card).insert(ScrollbarAttached);
    }
}

/// macOS-style presence: 3px idle strip; grows to 7px while the thumb is
/// hovered/dragged or the target scrolled recently, easing both ways.
pub(crate) fn style_scrollbars(
    time: Res<Time>,
    mut bars: Query<(Entity, &mut Node, &mut EditorScrollbar, &bevy::ui_widgets::Scrollbar)>,
    children: Query<&Children>,
    thumbs: Query<
        (&bevy::picking::hover::Hovered, Option<&bevy::ui_widgets::ScrollbarDragState>),
        With<bevy::ui_widgets::ScrollbarThumb>,
    >,
    scrolled: Query<(), Changed<ScrollPosition>>,
) {
    let now = time.elapsed_secs();
    for (entity, mut node, mut bar, scrollbar) in &mut bars {
        let mut engaged = false;
        // Thumb hover/drag anywhere under this bar.
        let mut stack = vec![entity];
        while let Some(current) = stack.pop() {
            if let Ok((hovered, drag)) = thumbs.get(current) {
                engaged |= hovered.0 || drag.is_some_and(|d| d.dragging);
            }
            if let Ok(kids) = children.get(current) {
                stack.extend(kids.iter());
            }
        }
        if scrolled.get(scrollbar.target).is_ok() {
            engaged = true;
        }
        if engaged {
            bar.last_active = now;
        }
        let target_width = if now - bar.last_active < 0.8 { 7.0 } else { 3.0 };
        let speed = (time.delta_secs() * 14.0).min(1.0);
        bar.width += (target_width - bar.width) * speed;
        node.width = px(bar.width);
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
        let target = if focused { style::color::accent() } else { style::color::TEXT_KEYS };
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
