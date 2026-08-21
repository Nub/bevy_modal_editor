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
    /// Asset problems have their own counter: an import that changes nothing
    /// about the level must still refresh this panel.
    last_asset_generation: u64,
}

#[derive(Component)]
pub(crate) struct ProblemsRoot;
#[derive(Component)]
pub(crate) struct ProblemsBody;

/// A clickable problem row targeting a scene entity.
#[derive(Component, Clone)]
pub(crate) struct ProblemRow(pub ProblemTarget);

/// What a row points at. An asset problem has no entity — inventing one, or
/// silently doing nothing on click, are both worse than saying so.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ProblemTarget {
    None,
    Entity(SceneId),
    Asset {
        path: String,
        uuid: Option<uuid::Uuid>,
    },
}

pub(crate) struct ProblemsFeature;

impl EditorFeature for ProblemsFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("level-problems", "Level Problems")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.action(
            ActionDef::new("level.problems", "Show Problems")
                .describe(
                    "Open level validation AND asset-pipeline problems; rows \
                     jump to the offender or reveal the asset",
                )
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
                        Text::new("PROBLEMS"),
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

/// Row click: an entity problem selects and frames the offender (the jump v1
/// users leaned on); an asset problem reveals the asset in the browser, which
/// is the same move for a thing that has no place in the level.
pub(crate) fn on_problem_row_press(
    press: On<Pointer<Press>>,
    rows: Query<&ProblemRow>,
    mut pending: ResMut<editor_core::selection::PendingSelect>,
    mut reveal: ResMut<crate::assets::AssetReveal>,
    mut actions: MessageWriter<ActionInvoked>,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    let Ok(row) = rows.get(press.entity) else {
        return;
    };
    match &row.0 {
        ProblemTarget::None => {}
        ProblemTarget::Entity(target) => {
            pending.0 = Some(*target);
            actions.write(ActionInvoked {
                action: ActionId::new_static("camera.frame"),
                args: None,
                source: InvocationSource::Palette,
            });
        }
        ProblemTarget::Asset {
            uuid: Some(uuid), ..
        } => reveal.0 = Some(*uuid),
        // A file that never got an identity has no row in the browser to
        // reveal. Say where it is rather than pretending to jump.
        ProblemTarget::Asset { path, uuid: None } => {
            feedback.write(editor_scene::SceneIoFeedback {
                message: path.clone(),
                success: false,
            });
        }
    }
}

pub(crate) fn sync_problems_ui(
    validation: Res<LevelValidation>,
    assets: Res<editor_scene::models::AssetProblems>,
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
    let generation_moved = validation.generation != state.last_generation
        || assets.generation != state.last_asset_generation;
    if state.open == *was_open && !generation_moved {
        return;
    }
    *was_open = state.open;
    state.last_generation = validation.generation;
    state.last_asset_generation = assets.generation;
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

    // Info-severity ingest noise is aggregated IN THE VIEW ONLY — 200 unclaimed
    // files must not be 200 rows. The stored list keeps one record per file, so
    // nothing is lost; this is presentation, not truncation.
    let mut ignored: std::collections::BTreeMap<String, usize> = Default::default();
    let mut asset_rows: Vec<(Severity, String, String, ProblemTarget)> = Vec::new();
    for problem in &assets.problems {
        if problem.severity == Severity::Info
            && problem.source == editor_api::validate::ProblemSource::Ingest
        {
            let ext = problem
                .path
                .rsplit_once('.')
                .map(|(_, e)| format!(".{e}"))
                .unwrap_or_else(|| "(no extension)".into());
            *ignored.entry(ext).or_insert(0) += 1;
            continue;
        }
        asset_rows.push((
            problem.severity,
            problem.source.label(problem.stage),
            format!("{} \u{b7} {}", problem.path, problem.message),
            ProblemTarget::Asset {
                path: problem.path.clone(),
                uuid: problem.uuid,
            },
        ));
    }
    if !ignored.is_empty() {
        let total: usize = ignored.values().sum();
        let breakdown = ignored
            .iter()
            .map(|(ext, n)| format!("{ext} \u{d7}{n}"))
            .collect::<Vec<_>>()
            .join(", ");
        asset_rows.push((
            Severity::Info,
            "import".into(),
            format!(
                "{total} file{} ignored ({breakdown})",
                if total == 1 { "" } else { "s" }
            ),
            ProblemTarget::None,
        ));
    }

    if validation.problems.is_empty() && asset_rows.is_empty() {
        commands.spawn((
            Text::new(format!(
                "no problems \u{b7} level valid \u{b7} {} asset file{} scanned \u{2713}",
                assets.scanned,
                if assets.scanned == 1 { "" } else { "s" }
            )),
            style::sans(&fonts, 11.0),
            TextColor(style::color::TEXT_DIM),
            ChildOf(body),
        ));
        return;
    }

    let section = |commands: &mut Commands, title: &str| {
        commands.spawn((
            Text::new(title.to_string()),
            style::mono(&fonts, 10.0),
            TextColor(style::color::TEXT_DIM),
            ChildOf(body),
        ));
    };

    if !validation.problems.is_empty() {
        section(&mut commands, "LEVEL");
        for problem in &validation.problems {
            spawn_problem_row(
                &mut commands,
                body,
                &fonts,
                problem.severity,
                problem.validator.as_str(),
                &problem.message,
                match problem.entity {
                    Some(id) => ProblemTarget::Entity(id),
                    None => ProblemTarget::None,
                },
            );
        }
    }
    if !asset_rows.is_empty() {
        section(&mut commands, "ASSETS");
        for (severity, source, message, target) in asset_rows {
            spawn_problem_row(
                &mut commands,
                body,
                &fonts,
                severity,
                &source,
                &message,
                target,
            );
        }
    }
}

/// One row, shared by both sections — a severity must never read two ways
/// depending on which list it came from.
#[allow(clippy::too_many_arguments)]
fn spawn_problem_row(
    commands: &mut Commands,
    body: Entity,
    fonts: &UiFonts,
    severity: Severity,
    source: &str,
    message: &str,
    target: ProblemTarget,
) {
    let (glyph, glyph_color) = match severity {
        Severity::Error => (style::glyph::ERROR, style::color::TEXT_WARN),
        Severity::Warning => (style::glyph::WARNING, style::color::TEXT_WARN),
        Severity::Info => (style::glyph::INFO, style::color::TEXT_DIM),
    };
    let row = commands
        .spawn((
            ProblemRow(target),
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
        style::mono(fonts, 11.0),
        TextColor(glyph_color),
        ChildOf(row),
    ));
    commands.spawn((
        Text::new(source.to_string()),
        style::no_wrap(),
        style::mono(fonts, 10.0),
        TextColor(style::color::TEXT_DIM),
        ChildOf(row),
    ));
    commands.spawn((
        Text::new(message.to_string()),
        style::sans(fonts, 11.0),
        TextColor(style::color::TEXT_KEYS),
        ChildOf(row),
    ));
}
