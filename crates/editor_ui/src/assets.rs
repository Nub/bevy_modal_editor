//! The asset browser (spec §6 "Asset browser (`editor_ui::assets`)").
//!
//! The pipeline records a great deal and, until this panel, showed almost none
//! of it: a stable uuid, a content hash, a measured bounding box, a validator's
//! objection, all of it reaching the designer as one line of a toast. This is
//! the surface where the work becomes visible — what was imported, how big it
//! is, and what the pipeline thought of it.
//!
//! Two rules shape it.
//!
//! **The browser is where things ARE; the palette is what things are CALLED.**
//! Searching stays in the palette — §7 mandates ONE palette engine and a second
//! fuzzy matcher is precisely the drift that rule exists to prevent — so `/`
//! here opens the palette rather than growing a second search field. The panel's
//! own job is structure: the directory tree the pack shipped, folded or open.
//!
//! **Placing goes through the palette's own function.** `palette::place_model`
//! is called from both, so a row can never place something the palette could
//! not, and neither can learn something the other does not.
//!
//! No thumbnails, deliberately and temporarily: handles are keyed by path and
//! the first loader of a path decides its settings, so a grid that eagerly
//! loaded source textures would gamma-decode every normal map in the project.
//! The spec section records the shape of the fix (a Process-stage output,
//! uploaded pathless); this cut adds zero new texture loads.

use bevy::prelude::*;
use bevy::ui::widget::Text;
use bevy::ui::{FlexDirection, UiRect, px};
use editor_api::validate::Severity;
use editor_core::prelude::*;
use editor_scene::models::{AssetProblems, EntryKind, ModelLibrary};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use crate::dock::PanelBody;
use crate::list::{ROW_HEIGHT, visible_window};
use crate::style::{self, UiFonts};

pub(crate) const ASSETS_PANEL: &str = "assets";

/// One line of the browser. Pure data, built by a pure function, so the whole
/// grouping and folding story is testable without a world.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AssetRow {
    Group {
        /// The full path key, e.g. `models/dungeon`. Folding is keyed on it.
        path: String,
        /// What the header prints — the last segment only.
        label: String,
        depth: usize,
        items: usize,
        collapsed: bool,
        worst: Option<Severity>,
        problems: usize,
    },
    Entry {
        uuid: Uuid,
        kind: EntryKind,
        name: String,
        depth: usize,
        extension: String,
        worst: Option<Severity>,
    },
}

/// Rank a severity so "worst" has a meaning.
fn rank(severity: Severity) -> u8 {
    match severity {
        Severity::Info => 0,
        Severity::Warning => 1,
        Severity::Error => 2,
    }
}

fn worse(a: Option<Severity>, b: Option<Severity>) -> Option<Severity> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if rank(a) >= rank(b) { a } else { b }),
        (some, None) | (None, some) => some,
    }
}

/// The directory an entry belongs to: everything before the file name.
fn group_of(asset_path: &str) -> String {
    match asset_path.rfind('/') {
        Some(cut) => asset_path[..cut].to_string(),
        None => String::new(),
    }
}

fn extension_of(asset_path: &str) -> String {
    asset_path
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_lowercase())
        .unwrap_or_default()
}

/// Build the visible row list.
///
/// ORDER IS NOT IMPOSED. `ingest` walks `models` then `textures` and sorts full
/// paths inside each, so `library.entries` is already directory-major and
/// name-minor; re-sorting here would be a second ordering that could disagree
/// with the one the pipeline established.
pub(crate) fn build_rows(
    library: &ModelLibrary,
    problems: &AssetProblems,
    collapsed: &HashSet<String>,
) -> Vec<AssetRow> {
    // Group in encounter order, so the pipeline's order survives.
    let mut order: Vec<String> = Vec::new();
    let mut members: HashMap<String, Vec<&editor_scene::models::ModelEntry>> = HashMap::new();
    for entry in &library.entries {
        let group = group_of(&entry.asset_path);
        if !members.contains_key(&group) {
            order.push(group.clone());
        }
        members.entry(group).or_default().push(entry);
    }

    let mut rows = Vec::new();
    for group in order {
        let entries = &members[&group];
        let depth = group.matches('/').count();
        let is_collapsed = collapsed.contains(&group);
        let mut worst = None;
        let mut count = 0usize;
        for entry in entries {
            let entry_worst = problems.worst_for(entry.uuid);
            if entry_worst.is_some() {
                count += problems.for_asset(entry.uuid).count();
            }
            worst = worse(worst, entry_worst);
        }
        rows.push(AssetRow::Group {
            label: group
                .rsplit('/')
                .next()
                .filter(|s| !s.is_empty())
                .unwrap_or("assets")
                .to_string(),
            path: group.clone(),
            depth,
            items: entries.len(),
            collapsed: is_collapsed,
            worst,
            problems: count,
        });
        if is_collapsed {
            continue;
        }
        for entry in entries {
            rows.push(AssetRow::Entry {
                uuid: entry.uuid,
                kind: entry.kind,
                name: entry.name.clone(),
                depth: depth + 1,
                extension: extension_of(&entry.asset_path),
                worst: problems.worst_for(entry.uuid),
            });
        }
    }
    rows
}

#[derive(Resource, Default)]
pub(crate) struct AssetsState {
    pub cursor: usize,
    pub collapsed: HashSet<String>,
    pub rows: Vec<AssetRow>,
    /// What the last rebuild was keyed on — never a serialize-to-compare.
    last_library: u64,
    last_problems: u64,
    dirty: bool,
    /// Set by a reveal: the row index is only known after the rebuild, so the
    /// request travels by uuid and the rebuild resolves it.
    pub pending_cursor: Option<Uuid>,
    /// `/` was pressed: searching is the palette's job, and the request
    /// crosses systems as state rather than as a re-emitted message.
    pub wants_search: bool,
    pub scroll: f32,
    pub view_height: f32,
}

impl AssetsState {
    pub fn cursor_uuid(&self) -> Option<Uuid> {
        match self.rows.get(self.cursor) {
            Some(AssetRow::Entry { uuid, .. }) => Some(*uuid),
            _ => None,
        }
    }
}

/// A request to show a particular asset — written by `asset.reveal` and by the
/// problems panel, consumed here.
#[derive(Resource, Default)]
pub(crate) struct AssetReveal(pub Option<Uuid>);

#[derive(Component, Clone)]
pub(crate) struct AssetRowNode(pub usize);
// The index is carried by the observer closure; the field exists so a probe
// (and any future keyboard-to-row mapping) can read a row back off the node.

pub(crate) struct AssetsFeature;

impl EditorFeature for AssetsFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("asset-browser", "Asset Browser")
    }

    fn register(&self, reg: &mut FeatureRegistry) {
        reg.panel(editor_api::panels::PanelDecl {
            id: editor_api::prelude::PanelId::new_static(ASSETS_PANEL),
            title: "Assets",
            placement: editor_api::panels::Placement::Bottom,
            context: editor_api::prelude::ContextId::new_static(ASSETS_PANEL),
            content: editor_api::panels::PanelContent::Custom,
            // Closed by default: it is the first tenant of the bottom dock and
            // an open one costs viewport height every session.
            default_open: false,
            // ...which is exactly why it needs a key. `Space w` is the
            // documented window/panel namespace (keymap §leader); a panel that
            // starts closed with no binding is a panel nobody finds.
            toggle_binding: Some("space w a"),
        });
        // Panel-scoped verbs, mirroring the hierarchy's set so the two lists
        // are one muscle memory.

        for (id, name, chord) in [
            ("assets.down", "Next Asset", "j"),
            ("assets.up", "Previous Asset", "k"),
            ("assets.bottom", "Last Asset", "shift+g"),
            ("assets.fold", "Fold Group", "h"),
            ("assets.unfold", "Unfold Group", "l"),
            ("assets.activate", "Place / Use Asset", "enter"),
            ("assets.search", "Search Assets", "/"),
        ] {
            reg.action(
                ActionDef::new(id, name)
                    .context(ASSETS_PANEL)
                    .bind(chord)
                    .hidden(),
            );
        }
        reg.action(
            ActionDef::new("asset.reveal", "Reveal Asset In Browser")
                .describe("Open the asset browser on the model the selection uses")
                .context("normal"),
        );
    }
}

/// Rebuild when — and only when — something it depends on moved. Both libraries
/// carry a generation counter for exactly this; comparing serialized state
/// would be the change detection §8 forbids.
pub(crate) fn watch_asset_sources(
    panel_states: Res<PanelStates>,
    library: Res<ModelLibrary>,
    problems: Res<AssetProblems>,
    focus: Res<PanelFocus>,
    mut state: ResMut<AssetsState>,
) {
    if panel_states.is_changed() {
        state.dirty = true;
    }
    if library.generation != state.last_library {
        state.last_library = library.generation;
        state.dirty = true;
    }
    if problems.generation != state.last_problems {
        state.last_problems = problems.generation;
        state.dirty = true;
    }
    if focus.is_changed() {
        state.dirty = true;
    }
}

pub(crate) fn collect_asset_actions(
    mut reader: MessageReader<ActionInvoked>,
    library: Res<ModelLibrary>,
    problems: Res<AssetProblems>,
    mut state: ResMut<AssetsState>,
    mut reveal: ResMut<AssetReveal>,
    selected: Query<&editor_scene::models::MeshRef, With<Selected>>,
    mut feedback: MessageWriter<editor_scene::SceneIoFeedback>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "assets.down" => {
                let last = state.rows.len().saturating_sub(1);
                state.cursor = (state.cursor + 1).min(last);
                state.dirty = true;
            }
            "assets.up" => {
                state.cursor = state.cursor.saturating_sub(1);
                state.dirty = true;
            }
            "assets.bottom" => {
                state.cursor = state.rows.len().saturating_sub(1);
                state.dirty = true;
            }
            // Fold on the cursor's own group, whether the cursor is the header
            // or one of its members — `h` on a row means "close the thing I am
            // inside", which is what it means in the hierarchy too.
            "assets.fold" | "assets.unfold" => {
                let fold = invoked.action.as_str() == "assets.fold";
                let group = group_at(&state, &library);
                if let Some(group) = group {
                    if fold {
                        state.collapsed.insert(group);
                    } else {
                        state.collapsed.remove(&group);
                    }
                    state.dirty = true;
                }
            }
            "asset.reveal" => match selected.iter().next() {
                Some(mesh_ref) => reveal.0 = Some(mesh_ref.0),
                None => {
                    feedback.write(editor_scene::SceneIoFeedback {
                        message: "select something that uses a model".into(),
                        success: false,
                    });
                }
            },
            "assets.search" => state.wants_search = true,
            "assets.activate" => {
                if let Some(uuid) = state.cursor_uuid() {
                    // Say what is wrong with it BEFORE placing it: an asset
                    // that failed validation still places (the pipeline never
                    // drops one), and arriving with no warning is how a broken
                    // mesh ends up in a level nobody suspects.
                    if let Some(problem) = problems
                        .for_asset(uuid)
                        .find(|p| p.severity == Severity::Error)
                    {
                        feedback.write(editor_scene::SceneIoFeedback {
                            message: format!("{}: {}", problem.stage.label(), problem.message),
                            success: false,
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

/// Which group the cursor is in: a header names itself, an entry names its
/// directory.
fn group_at(state: &AssetsState, library: &ModelLibrary) -> Option<String> {
    match state.rows.get(state.cursor)? {
        AssetRow::Group { path, .. } => Some(path.clone()),
        AssetRow::Entry { uuid, .. } => library.get(uuid).map(|entry| group_of(&entry.asset_path)),
    }
}

/// Enter places. It goes through the palette's own function so the browser can
/// never place something the palette could not — one transaction, one undo
/// entry, the selection landing on what arrived.
pub(crate) fn perform_asset_activate(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<AssetsState>,
    library: Res<ModelLibrary>,
    mut commands: Commands,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() != "assets.activate" {
            continue;
        }
        let Some(uuid) = state.cursor_uuid() else {
            continue;
        };
        // Only a MODEL places. A texture belongs to a material slot, and the
        // palette's texture arm already refuses when no slot is pending rather
        // than inventing a destination.
        if library.get(&uuid).map(|e| e.kind) != Some(EntryKind::Model) {
            continue;
        }
        commands.queue(move |world: &mut World| crate::palette::place_model(world, uuid));
    }
}

/// `/` hands searching to the palette rather than growing a second matcher.
pub(crate) fn open_palette_from_assets(
    mut state: ResMut<AssetsState>,
    mut writer: MessageWriter<ActionInvoked>,
) {
    // The request arrives as a FLAG rather than by re-reading ActionInvoked:
    // a system cannot both read and write one message type (the reader holds
    // the store shared while the writer wants it exclusively), and the panic
    // that produces is at startup, not at the keystroke.
    if !std::mem::take(&mut state.wants_search) {
        return;
    }
    writer.write(ActionInvoked {
        action: ActionId::new_static("palette.insert"),
        args: None,
        source: InvocationSource::Palette,
    });
}

/// Show an asset: open the panel, focus it, unfold what it is inside, and put
/// the cursor on it.
pub(crate) fn perform_reveal(
    mut reveal: ResMut<AssetReveal>,
    library: Res<ModelLibrary>,
    mut states: ResMut<PanelStates>,
    mut focus: ResMut<PanelFocus>,
    mut state: ResMut<AssetsState>,
) {
    let Some(uuid) = reveal.0.take() else {
        return;
    };
    let Some(entry) = library.get(&uuid) else {
        return;
    };
    let panel = editor_api::prelude::PanelId::new_static(ASSETS_PANEL);
    states.0.insert(panel.clone(), true);
    focus.0 = Some(panel);
    state.collapsed.remove(&group_of(&entry.asset_path));
    state.dirty = true;
    // The row index is only known after the rebuild, so ask for it by uuid.
    state.pending_cursor = Some(uuid);
}

/// The severity glyph and its colour — the same match the problems panel uses,
/// so one severity never reads two ways.
fn severity_mark(severity: Severity) -> (&'static str, Color) {
    match severity {
        Severity::Error => (style::glyph::ERROR, style::color::TEXT_WARN),
        Severity::Warning => (style::glyph::WARNING, style::color::TEXT_WARN),
        Severity::Info => (style::glyph::INFO, style::color::TEXT_DIM),
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn rebuild_assets(
    mut state: ResMut<AssetsState>,
    library: Res<ModelLibrary>,
    problems: Res<AssetProblems>,
    focus: Res<PanelFocus>,
    panel_states: Res<PanelStates>,
    body: Query<(Entity, &PanelBody)>,
    used: Query<&editor_scene::models::MeshRef>,
    fonts: Res<UiFonts>,
    settings: Res<EditorSettings>,
    mut commands: Commands,
) {
    if !state.dirty {
        return;
    }
    state.dirty = false;
    let Some((body_entity, _)) = body.iter().find(|(_, b)| b.0.as_str() == ASSETS_PANEL) else {
        return;
    };
    // A CLOSED panel holds no rows. The dock hides the card, but hidden UI
    // nodes still sit in the pointer's way — with the body populated, a click
    // anywhere over the closed bottom dock was swallowed by a row instead of
    // reaching the viewport, so selecting an object low on screen silently did
    // nothing. Building rows nobody can see was wrong twice over.
    if !panel_states.open(&editor_api::prelude::PanelId::new_static(ASSETS_PANEL)) {
        commands.entity(body_entity).despawn_related::<Children>();
        state.rows.clear();
        return;
    }
    let ui = settings.ui.clone();
    let rows = build_rows(&library, &problems, &state.collapsed);
    // A reveal asked for an asset, not a row number — resolve it now that the
    // rows exist.
    if let Some(wanted) = state.pending_cursor.take()
        && let Some(index) = rows
            .iter()
            .position(|row| matches!(row, AssetRow::Entry { uuid, .. } if *uuid == wanted))
    {
        state.cursor = index;
    }
    state.cursor = state.cursor.min(rows.len().saturating_sub(1));
    let cursor = state.cursor;
    let panel_focused = focus
        .0
        .as_ref()
        .is_some_and(|id| id.as_str() == ASSETS_PANEL);
    // Which assets the level actually uses — the passive half of
    // go-to-definition, and the answer to "is this one still needed".
    let in_use: HashMap<Uuid, usize> = used.iter().fold(HashMap::new(), |mut acc, mesh_ref| {
        *acc.entry(mesh_ref.0).or_insert(0) += 1;
        acc
    });

    commands.entity(body_entity).despawn_related::<Children>();
    let (first, last) = visible_window(state.scroll, state.view_height.max(1.0), rows.len());
    commands.entity(body_entity).with_children(|body_children| {
        if rows.is_empty() {
            // Which is a different sentence from "you have no assets": the scan
            // may simply never have run.
            let message = if problems.scanned == 0 {
                "no assets imported \u{b7} run Import Assets to scan assets/models"
            } else {
                "nothing the editor can load \u{b7} see the problems panel"
            };
            body_children.spawn((
                Text::new(message),
                style::sans(&fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
            ));
            return;
        }
        let mut list = body_children.spawn(Node {
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            ..default()
        });
        list.with_children(|body| {
            if first > 0 {
                body.spawn(Node {
                    height: px(first as f32 * ROW_HEIGHT),
                    flex_shrink: 0.0,
                    ..default()
                });
            }
            for (i, row) in rows.iter().enumerate().take(last).skip(first) {
                spawn_row(
                    body,
                    i,
                    row,
                    cursor,
                    panel_focused,
                    &library,
                    &in_use,
                    &fonts,
                    &ui,
                );
            }
            if last < rows.len() {
                body.spawn(Node {
                    height: px((rows.len() - last) as f32 * ROW_HEIGHT),
                    flex_shrink: 0.0,
                    ..default()
                });
            }
        });
    });
    state.rows = rows;
}

#[allow(clippy::too_many_arguments)]
fn spawn_row(
    body: &mut bevy::ecs::hierarchy::ChildSpawnerCommands,
    index: usize,
    row: &AssetRow,
    cursor: usize,
    panel_focused: bool,
    library: &ModelLibrary,
    in_use: &HashMap<Uuid, usize>,
    fonts: &UiFonts,
    ui: &editor_core::settings::UiSettings,
) {
    let is_cursor = index == cursor;
    let depth = match row {
        AssetRow::Group { depth, .. } | AssetRow::Entry { depth, .. } => *depth,
    };
    let mut entity = body.spawn((
        AssetRowNode(index),
        Node {
            align_items: AlignItems::Center,
            column_gap: px(style::space::XS),
            height: px(ROW_HEIGHT),
            padding: UiRect {
                left: px(style::space::S + depth as f32 * style::space::M),
                right: px(style::space::S),
                ..default()
            },
            border_radius: BorderRadius::all(px(style::radius::S)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(if is_cursor && panel_focused {
            style::color::selection()
        } else if is_cursor {
            // Authored for LINEAR blending, the same wash the hierarchy uses
            // for an unfocused cursor.
            style::color::selection().with_alpha(0.03)
        } else {
            Color::NONE
        }),
    ));
    entity.observe(
        move |press: On<Pointer<Press>>,
              rows_q: Query<&AssetRowNode>,
              mut state: ResMut<AssetsState>,
              mut focus: ResMut<PanelFocus>| {
            // Read the index off the NODE rather than the closure: the node is
            // what the rebuild actually stamped, so a stale capture cannot put
            // the cursor on a row that moved.
            if let Ok(node) = rows_q.get(press.entity) {
                state.cursor = node.0;
                state.dirty = true;
                // Clicking a row means you are working in this panel — the same
                // thing clicking a hierarchy row means.
                focus.0 = Some(editor_api::prelude::PanelId::new_static(ASSETS_PANEL));
            }
        },
    );
    entity.with_children(|node| match row {
        AssetRow::Group {
            label,
            items,
            collapsed,
            worst,
            problems,
            ..
        } => {
            node.spawn((
                Text::new(if *collapsed {
                    style::CHEVRON_RIGHT
                } else {
                    style::CHEVRON_DOWN
                }),
                style::mono(fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
            ));
            node.spawn((
                Text::new(label.to_uppercase()),
                style::sans_medium(fonts, ui.font_size_s),
                TextColor(style::color::TEXT_BRIGHT),
            ));
            node.spawn((
                Text::new(format!("{items}")),
                style::mono(fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
            ));
            node.spawn((Node {
                flex_grow: 1.0,
                ..default()
            },));
            // Folded, the panel becomes a one-screen health report.
            if let Some(worst) = worst {
                let (glyph, colour) = severity_mark(*worst);
                node.spawn((
                    Text::new(format!("{glyph} {problems}")),
                    style::mono(fonts, ui.font_size_s),
                    TextColor(colour),
                ));
            }
        }
        AssetRow::Entry {
            uuid,
            kind,
            name,
            extension,
            worst,
            ..
        } => {
            match worst {
                Some(severity) => {
                    let (glyph, colour) = severity_mark(*severity);
                    node.spawn((
                        Text::new(glyph),
                        style::mono(fonts, ui.font_size_s),
                        TextColor(colour),
                    ));
                }
                // A blank of the same width, so names still line up.
                None => {
                    node.spawn((
                        Text::new(" "),
                        style::mono(fonts, ui.font_size_s),
                        TextColor(style::color::TEXT_DIM),
                    ));
                }
            }
            node.spawn((
                Text::new(match kind {
                    EntryKind::Model => style::glyph::MODEL,
                    EntryKind::Texture => style::glyph::TEXTURE,
                }),
                style::mono(fonts, ui.font_size_s),
                TextColor(style::color::TEXT_DIM),
            ));
            node.spawn((
                Text::new(name.clone()),
                style::sans(fonts, ui.font_size_m),
                TextColor(style::color::TEXT_BRIGHT),
            ));
            node.spawn((Node {
                flex_grow: 1.0,
                ..default()
            },));
            match kind {
                EntryKind::Model => {
                    // The Process stage's own measurement, rendered by the same
                    // function the palette uses so the two cannot disagree.
                    let bounds = library.get(uuid).and_then(|entry| entry.bounds);
                    let (text, colour) = crate::palette::size_line(bounds);
                    node.spawn((
                        Text::new(text),
                        style::mono(fonts, ui.font_size_s),
                        TextColor(colour),
                    ));
                    if let Some(bounds) = bounds {
                        node.spawn((
                            Text::new(format!("{} tris", bounds.triangles)),
                            style::mono(fonts, ui.font_size_s),
                            TextColor(style::color::TEXT_DIM),
                        ));
                    }
                }
                EntryKind::Texture => {
                    // Pixel dimensions are NOT shown: they are recorded
                    // nowhere, and reading them means loading the source, which
                    // is the handle-settings race the spec section describes.
                }
            }
            let uses = in_use.get(uuid).copied().unwrap_or(0);
            if uses > 0 {
                node.spawn((
                    Text::new(format!("\u{b7} {uses} in scene")),
                    style::mono(fonts, ui.font_size_s),
                    TextColor(style::color::accent()),
                ));
            }
            node.spawn((
                Text::new(extension.clone()),
                style::mono(fonts, ui.font_size_s),
                TextColor(style::color::TEXT_KEYS),
            ));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use editor_api::validate::{AssetProblem, ProblemSource, Stage};
    use editor_scene::models::ModelEntry;

    fn entry(path: &str, kind: EntryKind) -> ModelEntry {
        ModelEntry {
            uuid: Uuid::new_v4(),
            kind,
            name: path
                .rsplit('/')
                .next()
                .and_then(|f| f.split('.').next())
                .unwrap_or("asset")
                .to_string(),
            asset_path: path.to_string(),
            content_hash: String::new(),
            bounds: None,
        }
    }

    fn library(entries: Vec<ModelEntry>) -> ModelLibrary {
        ModelLibrary {
            entries,
            ..default()
        }
    }

    fn problem(uuid: Uuid, severity: Severity) -> AssetProblem {
        AssetProblem {
            stage: Stage::Validate,
            source: ProblemSource::Ingest,
            severity,
            path: String::new(),
            uuid: Some(uuid),
            message: "x".into(),
        }
    }

    #[test]
    fn a_flat_library_groups_by_its_two_roots() {
        let lib = library(vec![
            entry("models/barrel.glb", EntryKind::Model),
            entry("textures/rust.png", EntryKind::Texture),
        ]);
        let rows = build_rows(&lib, &AssetProblems::default(), &HashSet::new());
        let groups: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                AssetRow::Group { path, .. } => Some(path.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(groups, vec!["models", "textures"]);
        assert_eq!(rows.len(), 4, "two headers and two entries");
    }

    /// `models/dungeon/walls/*.glb` is the shape every marketplace ships, and
    /// the reason ingest walks recursively in the first place.
    #[test]
    fn a_subdirectory_becomes_its_own_group_at_depth() {
        let lib = library(vec![
            entry("models/barrel.glb", EntryKind::Model),
            entry("models/dungeon/walls/wall_a.glb", EntryKind::Model),
        ]);
        let rows = build_rows(&lib, &AssetProblems::default(), &HashSet::new());
        let nested = rows
            .iter()
            .find_map(|r| match r {
                AssetRow::Group { path, depth, .. } if path == "models/dungeon/walls" => {
                    Some(*depth)
                }
                _ => None,
            })
            .expect("the nested directory got no group");
        assert_eq!(nested, 2, "depth is the directory depth, not a counter");
    }

    /// The pipeline already sorted; a second ordering here could disagree with
    /// the one the walk established.
    #[test]
    fn stored_order_is_preserved() {
        let lib = library(vec![
            entry("models/zebra.glb", EntryKind::Model),
            entry("models/aardvark.glb", EntryKind::Model),
        ]);
        let rows = build_rows(&lib, &AssetProblems::default(), &HashSet::new());
        let names: Vec<&str> = rows
            .iter()
            .filter_map(|r| match r {
                AssetRow::Entry { name, .. } => Some(name.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(names, vec!["zebra", "aardvark"], "the rows were re-sorted");
    }

    #[test]
    fn a_folded_group_keeps_its_header_and_hides_its_members() {
        let lib = library(vec![entry("models/barrel.glb", EntryKind::Model)]);
        let mut collapsed = HashSet::new();
        collapsed.insert("models".to_string());
        let rows = build_rows(&lib, &AssetProblems::default(), &collapsed);
        assert_eq!(rows.len(), 1);
        assert!(matches!(
            &rows[0],
            AssetRow::Group {
                collapsed: true,
                items: 1,
                ..
            }
        ));
    }

    /// Folded, the panel is a one-screen health report — which only works if a
    /// header carries the worst thing under it, not the first or the last.
    #[test]
    fn a_group_carries_the_worst_severity_beneath_it() {
        let a = entry("models/a.glb", EntryKind::Model);
        let b = entry("models/b.glb", EntryKind::Model);
        let (a_id, b_id) = (a.uuid, b.uuid);
        let lib = library(vec![a, b]);
        let problems = AssetProblems {
            problems: vec![
                problem(a_id, Severity::Info),
                problem(b_id, Severity::Error),
                problem(b_id, Severity::Warning),
            ],
            ..default()
        };
        let rows = build_rows(&lib, &problems, &HashSet::new());
        match &rows[0] {
            AssetRow::Group {
                worst, problems: n, ..
            } => {
                assert_eq!(*worst, Some(Severity::Error), "a lesser problem won");
                assert_eq!(*n, 3, "the count is every problem under the group");
            }
            other => panic!("expected a group header, got {other:?}"),
        }
    }

    #[test]
    fn an_asset_nobody_complained_about_carries_no_mark() {
        let lib = library(vec![entry("models/barrel.glb", EntryKind::Model)]);
        let rows = build_rows(&lib, &AssetProblems::default(), &HashSet::new());
        assert!(matches!(&rows[1], AssetRow::Entry { worst: None, .. }));
    }

    #[test]
    fn an_empty_library_has_no_rows_at_all() {
        let rows = build_rows(&library(vec![]), &AssetProblems::default(), &HashSet::new());
        assert!(rows.is_empty(), "an empty library invented a group");
    }
}
