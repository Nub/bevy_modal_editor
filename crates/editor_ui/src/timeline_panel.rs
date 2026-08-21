//! The track view (spec §9 Animation, Layer 1).
//!
//! Keys were authorable and invisible: what was keyed, and when, could only be
//! learned by reading `timeline.ron`. A timeline you cannot see is one you
//! cannot correct — you can add a key but never notice you put it in the wrong
//! place, which is most of what authoring an animation consists of.
//!
//! Rows are rebuilt when the timeline changes; the playhead moves every frame
//! and is therefore a position update rather than a rebuild, because rebuilding
//! a panel sixty times a second to move one line is how a UI starts dropping
//! frames.

use bevy::prelude::*;
use bevy::ui::{Val, percent, px};
use editor_core::prelude::*;
use editor_scene::anim::{Playhead, Timeline};

use crate::style;
use crate::style::UiFonts;

#[derive(Component)]
pub(crate) struct TimelinePanel;
#[derive(Component)]
pub(crate) struct TimelineBody;
#[derive(Component)]
pub(crate) struct TimelineClock;
/// The line showing where time is. Moved, never rebuilt.
#[derive(Component)]
pub(crate) struct TimelineCursor;
/// One key mark, carrying the moment it stands for so a test can check that
/// where it SITS matches when it happens.
#[derive(Component, Clone, Copy)]
pub(crate) struct KeyMark {
    pub time: f32,
    pub fraction: f32,
}
/// The clickable strip of one track: pressing it scrubs.
#[derive(Component)]
pub(crate) struct TrackStrip;
/// One event mark, carrying its moment and name so a test can check that where
/// it sits matches when it fires.
#[derive(Component, Clone)]
pub(crate) struct EventMark {
    pub time: f32,
    pub fraction: f32,
    pub name: String,
}

#[derive(Resource, Default)]
pub(crate) struct TimelinePanelState {
    pub open: bool,
    /// Generation the rows were built from, so they rebuild when it moves.
    pub built_generation: u64,
}

pub(crate) struct TimelinePanelFeature;

impl EditorFeature for TimelinePanelFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("timeline-panel", "Timeline")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.action(
            ActionDef::new("timeline.toggle", "Toggle Timeline")
                .describe("Show the tracks, their keys, and where time is")
                .context("normal")
                .bind("space t t"),
        );
        reg.action(
            ActionDef::new("timeline.event", "Add Timeline Event")
                .describe("Mark this moment with a name the game can react to")
                .context("normal")
                .bind("space t e"),
        );
    }
}

pub(crate) fn spawn_timeline_panel(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    settings: Res<EditorSettings>,
) {
    let ui = settings.ui.clone();
    let root = commands
        .spawn((
            TimelinePanel,
            crate::appear::FloatingSurface::default(),
            Node {
                position_type: PositionType::Absolute,
                // A timeline belongs along the bottom, above the status bar and
                // clear of the side docks. It said "clear of the side docks"
                // while hardcoding 228 against a left dock 280 wide, so it drew
                // over the last ~50px of the hierarchy the whole time. Derived
                // from the docks now, so the next width change cannot re-break
                // it — and so the bottom dock, which the asset browser is the
                // first tenant of, is not covered either.
                left: px(ui.dock_left_width + style::space::M),
                right: px(ui.dock_right_width + style::space::M),
                bottom: px(style::BAR_HEIGHT + style::space::M),
                max_height: px(200.0),
                flex_direction: FlexDirection::Column,
                row_gap: px(style::space::XS),
                padding: UiRect::all(px(style::space::M)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(style::radius::L)),
                overflow: bevy::ui::Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgb(0.125, 0.122, 0.117)),
            BorderColor::all(style::HAIRLINE),
            style::floating_shadow(),
            GlobalZIndex(70),
            Visibility::Hidden,
        ))
        .id();
    let header = commands
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: px(style::space::S),
                ..default()
            },
            ChildOf(root),
        ))
        .id();
    commands.spawn((
        Text::new("TIMELINE"),
        style::sans(&fonts, 10.0),
        TextColor(style::color::TEXT_DIM),
        ChildOf(header),
    ));
    commands.spawn((
        TimelineClock,
        Text::new(String::new()),
        style::mono(&fonts, 10.0),
        TextColor(style::color::TEXT_KEYS),
        ChildOf(header),
    ));
    commands.spawn((
        TimelineBody,
        Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(2.0),
            ..default()
        },
        ChildOf(root),
    ));
}

pub(crate) fn handle_timeline_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut state: ResMut<TimelinePanelState>,
    mut prompt: ResMut<editor_prefabs::authoring::GroupPrompt>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "timeline.toggle" => state.open = !state.open,
            // Through THE name prompt, the same surface prefab naming and
            // material renaming use.
            "timeline.event" => {
                prompt.open = true;
                prompt.purpose = editor_prefabs::authoring::PromptPurpose::TimelineEvent;
            }
            _ => {}
        }
    }
}

/// Consume the name the prompt collected and mark this moment with it.
pub(crate) fn commit_timeline_event(
    mut commit: ResMut<editor_prefabs::authoring::GroupCommit>,
    prompt: Res<editor_prefabs::authoring::GroupPrompt>,
    playhead: Res<Playhead>,
    mut timeline: ResMut<Timeline>,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    if prompt.purpose != editor_prefabs::authoring::PromptPurpose::TimelineEvent {
        return;
    }
    let Some(name) = commit.0.take() else { return };
    let name = name.trim().to_string();
    if name.is_empty() {
        return;
    }
    let at = playhead.time;
    timeline.events.push(editor_scene::anim::EventMarker {
        time: at,
        name: name.clone(),
    });
    // Sorted, so the track view reads left to right and crossing walks in order.
    timeline.events.sort_by(|a, b| a.time.total_cmp(&b.time));
    timeline.generation += 1;
    feedback.write(editor_scene::SceneIoFeedback {
        message: format!("{name} at {at:.2}s"),
        success: true,
    });
}

/// Rebuild the rows when the tracks change — NOT when time moves.
pub(crate) fn sync_timeline_rows(
    mut commands: Commands,
    fonts: Res<UiFonts>,
    timeline: Res<Timeline>,
    mut state: ResMut<TimelinePanelState>,
    editor: Res<EditorState>,
    names: Query<&Name>,
    index: Res<editor_api::edits::SceneIndex>,
    panels: Query<Entity, With<TimelinePanel>>,
    bodies: Query<Entity, With<TimelineBody>>,
    mut visibility: Query<&mut Visibility, With<TimelinePanel>>,
    mut nodes: Query<&mut Node, With<TimelinePanel>>,
    settings: Res<EditorSettings>,
    panel_states: Res<PanelStates>,
    catalog: Res<PanelCatalog>,
) {
    // Sit ABOVE the bottom dock when something is in it, rather than over it.
    // The rule, not the number: any future bottom-dock tenant keeps working.
    let bottom_dock = catalog
        .in_placement(editor_api::panels::Placement::Bottom)
        .any(|decl| panel_states.open(&decl.id));
    let lift = if bottom_dock {
        settings.ui.dock_bottom_height
    } else {
        0.0
    };
    for mut node in &mut nodes {
        let target = px(style::BAR_HEIGHT + style::space::M + lift);
        if node.bottom != target {
            node.bottom = target;
        }
    }
    let want = state.open && editor.active;
    for mut visible in &mut visibility {
        let target = if want {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
        if *visible != target {
            *visible = target;
        }
    }
    if !want {
        return;
    }
    if state.built_generation == timeline.generation && !state.is_changed() {
        return;
    }
    state.built_generation = timeline.generation;
    let (Ok(_panel), Ok(body)) = (panels.single(), bodies.single()) else {
        return;
    };
    commands.entity(body).despawn_related::<Children>();

    let duration = timeline.duration().max(f32::EPSILON);
    for track in &timeline.tracks {
        let row = commands
            .spawn((
                Node {
                    height: px(18.0),
                    align_items: AlignItems::Center,
                    column_gap: px(style::space::S),
                    flex_shrink: 0.0,
                    ..default()
                },
                ChildOf(body),
            ))
            .id();
        // Name the thing, not the uuid: "crate · translation.y" is what the
        // designer keyed, "8f2a…" is what the file happens to call it.
        let subject = index
            .get(&track.target)
            .and_then(|entity| names.get(entity).ok())
            .map(|name| name.as_str().to_string())
            .unwrap_or_else(|| "entity".into());
        commands.spawn((
            Text::new(format!("{subject} \u{b7} {}", track.path)),
            style::no_wrap(),
            style::sans(&fonts, 10.0),
            TextColor(style::color::TEXT_KEYS),
            Node {
                width: px(150.0),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(row),
        ));
        let strip = commands
            .spawn((
                TrackStrip,
                Node {
                    flex_grow: 1.0,
                    height: percent(100.0),
                    position_type: PositionType::Relative,
                    border_radius: BorderRadius::all(px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.04)),
                ChildOf(row),
            ))
            .observe(on_strip_press)
            .id();
        for key in &track.keys {
            let fraction = (key.time / duration).clamp(0.0, 1.0);
            commands.spawn((
                KeyMark {
                    time: key.time,
                    fraction,
                },
                Node {
                    position_type: PositionType::Absolute,
                    left: percent(fraction * 100.0),
                    top: px(5.0),
                    width: px(7.0),
                    height: px(7.0),
                    // A diamond is the keyframe glyph everywhere, and it stays
                    // legible at seven pixels where a label would not.
                    border_radius: BorderRadius::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(style::color::accent()),
                bevy::picking::Pickable::IGNORE,
                ChildOf(strip),
            ));
        }
    }
    // Events get their own row, above the tracks they punctuate: they belong to
    // the timeline rather than to any one property, and a designer reads them
    // as "what happens when", not "what moves".
    if !timeline.events.is_empty() {
        let row = commands
            .spawn((
                Node {
                    height: px(18.0),
                    align_items: AlignItems::Center,
                    column_gap: px(style::space::S),
                    flex_shrink: 0.0,
                    ..default()
                },
                ChildOf(body),
            ))
            .id();
        commands.spawn((
            Text::new("events".to_string()),
            style::no_wrap(),
            style::sans(&fonts, 10.0),
            TextColor(style::color::TEXT_DIM),
            Node {
                width: px(150.0),
                flex_shrink: 0.0,
                ..default()
            },
            ChildOf(row),
        ));
        let strip = commands
            .spawn((
                TrackStrip,
                Node {
                    flex_grow: 1.0,
                    height: percent(100.0),
                    position_type: PositionType::Relative,
                    border_radius: BorderRadius::all(px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.04)),
                ChildOf(row),
            ))
            .observe(on_strip_press)
            .id();
        for event in &timeline.events {
            let fraction = (event.time / duration).clamp(0.0, 1.0);
            let mark = commands
                .spawn((
                    EventMark {
                        time: event.time,
                        fraction,
                        name: event.name.clone(),
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(fraction * 100.0),
                        top: px(2.0),
                        bottom: px(2.0),
                        // A bar, not a diamond: an event is a moment something
                        // HAPPENS at, and it must not be mistaken for a key.
                        width: px(2.0),
                        ..default()
                    },
                    BackgroundColor(style::color::TEXT_BRIGHT),
                    bevy::picking::Pickable::IGNORE,
                    ChildOf(strip),
                ))
                .id();
            // Named in place: an event nobody can identify is a tick mark.
            commands.spawn((
                Text::new(event.name.clone()),
                style::no_wrap(),
                style::sans(&fonts, 9.0),
                TextColor(style::color::TEXT_KEYS),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(4.0),
                    top: px(1.0),
                    ..default()
                },
                bevy::picking::Pickable::IGNORE,
                ChildOf(mark),
            ));
        }
    }
    // The cursor lives above the rows and spans them.
    commands.spawn((
        TimelineCursor,
        Node {
            position_type: PositionType::Absolute,
            left: percent(0.0),
            top: px(0.0),
            bottom: px(0.0),
            width: px(1.0),
            ..default()
        },
        BackgroundColor(style::color::accent()),
        bevy::picking::Pickable::IGNORE,
        ChildOf(body),
    ));
}

/// Move the cursor and update the clock. Every frame, cheaply: this is why the
/// rows are not rebuilt here.
pub(crate) fn sync_timeline_cursor(
    timeline: Res<Timeline>,
    playhead: Res<Playhead>,
    state: Res<TimelinePanelState>,
    mut cursors: Query<&mut Node, With<TimelineCursor>>,
    mut clocks: Query<&mut Text, With<TimelineClock>>,
) {
    if !state.open {
        return;
    }
    let duration = timeline.duration();
    let fraction = if duration > 0.0 {
        (playhead.time / duration).clamp(0.0, 1.0)
    } else {
        0.0
    };
    for mut node in &mut cursors {
        let left = Val::Percent(fraction * 100.0);
        if node.left != left {
            node.left = left;
        }
    }
    for mut text in &mut clocks {
        let next = format!("{:.2}s / {:.2}s", playhead.time, duration);
        if text.0 != next {
            text.0 = next;
        }
    }
}

/// Press a track strip to scrub there — the timeline is a control, not a
/// readout.
pub(crate) fn on_strip_press(
    press: On<Pointer<Press>>,
    strips: Query<(&ComputedNode, &bevy::ui::UiGlobalTransform), With<TrackStrip>>,
    timeline: Res<Timeline>,
    mut playhead: ResMut<Playhead>,
) {
    let Ok((node, transform)) = strips.get(press.entity) else {
        return;
    };
    let duration = timeline.duration();
    if duration <= 0.0 {
        return;
    }
    let width = node.size().x;
    if width <= 0.0 {
        return;
    }
    // The press position is global; the strip's transform gives its centre.
    let left = transform.translation.x - width * 0.5;
    let fraction = ((press.pointer_location.position.x - left) / width).clamp(0.0, 1.0);
    playhead.time = fraction * duration;
    playhead.playing = false;
}
