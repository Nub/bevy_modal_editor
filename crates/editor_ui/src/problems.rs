//! The level-problems surface (v1 parity, follows level_validation): every
//! failing rule as a row — severity glyph, rule, message — and entity-shaped
//! problems JUMP: clicking a row selects the offender and frames it. Opens on
//! demand (`level.problems`), auto-opens when an explicit `level.validate`
//! finds problems, closes on empty-handed Escape (one layer per press).

use bevy::prelude::*;
use bevy::ui::{PositionType, px};
use editor_api::validate::Severity;
use editor_core::prelude::*;
use editor_scene::level_validation::LevelValidation;

use crate::appear::FloatingSurface;
use crate::style::{self, UiFonts};

#[derive(Resource, Default)]
pub(crate) struct ProblemsState {
    pub open: bool,
    /// An explicit validate just ran — open when its results land.
    pending_auto_open: bool,
    last_generation: u64,
}

#[derive(Component)]
pub(crate) struct ProblemsRoot;
#[derive(Component)]
pub(crate) struct ProblemsBody;

/// A clickable problem row targeting a scene entity.
#[derive(Component, Clone, Copy)]
pub(crate) struct ProblemRow(pub Option<SceneId>);

pub(crate) struct ProblemsFeature;

impl EditorFeature for ProblemsFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("level-problems", "Level Problems")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.action(
            ActionDef::new("level.problems", "Show Level Problems")
                .describe("Open the level validation results; rows jump to the offender")
                .context("normal"),
        );
    }
}

pub(crate) fn collect_problem_actions(
    mut reader: MessageReader<ActionInvoked>,
    selected: Query<(), With<Selected>>,
    escape_from_capture: Res<editor_core::resolver::EscapeFromCapture>,
    mut state: ResMut<ProblemsState>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "level.problems" => state.open = !state.open,
            // An explicit validate WITH problems deserves the panel, not a
            // "see log" flash.
            "level.validate" => state.pending_auto_open = true,
            "core.escape-home" if state.open && !escape_from_capture.0 && selected.is_empty() => {
                state.open = false;
            }
            _ => {}
        }
    }
}

pub(crate) fn spawn_problems_root(mut commands: Commands, fonts: Res<UiFonts>) {
    commands
        .spawn((
            ProblemsRoot,
            FloatingSurface::default(),
            Node {
                position_type: PositionType::Absolute,
                left: px(style::space::M),
                bottom: px(style::BAR_HEIGHT + style::space::M),
                width: px(460.0),
                max_height: px(300.0),
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
        .with_children(|root| {
            root.spawn((Node {
                align_items: AlignItems::Center,
                column_gap: px(style::space::S),
                ..default()
            },))
                .with_children(|header| {
                    header.spawn((
                        Text::new("LEVEL PROBLEMS"),
                        style::sans_medium(&fonts, 10.0),
                        TextColor(style::color::TEXT_DIM),
                    ));
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    header.spawn((
                        Text::new("\u{238b} close"),
                        style::mono(&fonts, 10.0),
                        TextColor(style::color::TEXT_DIM),
                    ));
                });
            root.spawn((
                ProblemsBody,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(2.0),
                    ..default()
                },
            ));
        });
}

/// Row click: select the offender (the jump v1 users leaned on) and frame it.
pub(crate) fn on_problem_row_press(
    press: On<Pointer<Press>>,
    rows: Query<&ProblemRow>,
    mut pending: ResMut<editor_core::selection::PendingSelect>,
    mut actions: MessageWriter<ActionInvoked>,
) {
    let Ok(row) = rows.get(press.entity) else {
        return;
    };
    let Some(target) = row.0 else { return };
    pending.0 = Some(target);
    actions.write(ActionInvoked {
        action: ActionId::new_static("camera.frame"),
        args: None,
        source: InvocationSource::Palette,
    });
}

pub(crate) fn sync_problems_ui(
    validation: Res<LevelValidation>,
    fonts: Res<UiFonts>,
    mut state: ResMut<ProblemsState>,
    mut root: Query<&mut Visibility, With<ProblemsRoot>>,
    body: Query<Entity, With<ProblemsBody>>,
    mut commands: Commands,
    mut was_open: Local<bool>,
) {
    // Auto-open once the demanded validation's results land (generation moved).
    if state.pending_auto_open && validation.generation != state.last_generation {
        state.pending_auto_open = false;
        if !validation.problems.is_empty() {
            state.open = true;
        }
    }
    let generation_moved = validation.generation != state.last_generation;
    if state.open == *was_open && !generation_moved {
        return;
    }
    *was_open = state.open;
    state.last_generation = validation.generation;
    for mut visibility in &mut root {
        *visibility = if state.open {
            Visibility::Visible
        } else {
            Visibility::Hidden
        };
    }
    if !state.open {
        return;
    }
    let Ok(body) = body.single() else { return };
    commands.entity(body).despawn_related::<Children>();

    if validation.problems.is_empty() {
        commands.spawn((
            Text::new("no problems — the level is valid \u{2713}"),
            style::sans(&fonts, 11.0),
            TextColor(style::color::TEXT_DIM),
            ChildOf(body),
        ));
        return;
    }
    for problem in &validation.problems {
        let (glyph, glyph_color) = match problem.severity {
            Severity::Error => (style::glyph::ERROR, style::color::TEXT_WARN),
            Severity::Warning => (style::glyph::WARNING, style::color::TEXT_WARN),
            Severity::Info => (style::glyph::INFO, style::color::TEXT_DIM),
        };
        let row = commands
            .spawn((
                ProblemRow(problem.entity),
                Node {
                    align_items: AlignItems::Center,
                    column_gap: px(style::space::S),
                    padding: UiRect::axes(px(style::space::S), px(3.0)),
                    border_radius: BorderRadius::all(px(style::radius::S)),
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(Color::srgba(1.0, 1.0, 1.0, 0.03)),
                ChildOf(body),
            ))
            .observe(on_problem_row_press)
            .id();
        commands.spawn((
            Text::new(glyph),
            style::mono(&fonts, 11.0),
            TextColor(glyph_color),
            ChildOf(row),
        ));
        commands.spawn((
            Text::new(problem.validator.as_str().to_string()),
            style::no_wrap(),
            style::mono(&fonts, 10.0),
            TextColor(style::color::TEXT_DIM),
            ChildOf(row),
        ));
        commands.spawn((
            Text::new(problem.message.clone()),
            style::sans(&fonts, 11.0),
            TextColor(style::color::TEXT_KEYS),
            ChildOf(row),
        ));
    }
}
