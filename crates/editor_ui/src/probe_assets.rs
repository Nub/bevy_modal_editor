//! Asset browser probe (`ASSETS_PROBE=1`, spec §6 "Asset browser").
//!
//! The panel exists to make the pipeline's work visible, so the assertions are
//! about what actually reaches a designer's eyes: the dock opens where the
//! arithmetic says it should, a row per imported asset materializes, a model
//! row carries the size the Process stage measured, and a file that fails
//! validation is marked as failing — with the same record reaching the problems
//! panel and the status bar.
//!
//! It also owns the geometry regression the browser creates. The timeline is a
//! floating surface that hardcoded its own left edge; with a bottom dock now
//! real, an unchecked timeline would draw straight over this panel. The check
//! is against all three dock rectangles, which is the check the existing
//! timeline probe never made.

use bevy::prelude::*;
use editor_api::validate::{ProblemSource, Severity};
use editor_core::prelude::*;
use editor_scene::models::{AssetProblems, ModelLibrary};

use crate::probe_user::shot;

#[derive(Resource, Default)]
pub(crate) struct AssetsProbe {
    frame: u32,
    failures: Vec<String>,
}

fn check(world: &mut World, ok: bool, what: &str) {
    if ok {
        info!("ASSETS-PROBE PASS: {what}");
    } else {
        error!("ASSETS-PROBE FAIL: {what}");
        world
            .resource_mut::<AssetsProbe>()
            .failures
            .push(what.to_string());
    }
}

fn invoke(world: &mut World, action: &'static str) {
    world.write_message(ActionInvoked {
        action: ActionId::new_static(action),
        args: None,
        source: InvocationSource::Test,
    });
}

/// The computed screen rect of a UI node, or `None` if it has no layout yet.
/// The computed screen rect of a UI node, in LOGICAL pixels.
///
/// `ComputedNode::size()` is physical, and `UiGlobalTransform` is logical — on
/// a 2x display, mixing them silently doubles every height. Everything the
/// settings and the layout code speak is logical, so convert here rather than
/// leaving the trap for the next reader.
fn rect_of(world: &mut World, entity: Entity) -> Option<Rect> {
    let node = world.get::<ComputedNode>(entity)?;
    let size = node.size() * node.inverse_scale_factor();
    let transform = world.get::<bevy::ui::UiGlobalTransform>(entity)?;
    let center = transform.translation;
    Some(Rect::from_center_size(Vec2::new(center.x, center.y), size))
}

fn dock_rects(world: &mut World) -> Vec<(String, Rect)> {
    let docks: Vec<(Entity, String)> = world
        .query::<(Entity, &crate::dock::DockRoot, &Visibility)>()
        .iter(world)
        .filter(|(_, _, visibility)| **visibility != Visibility::Hidden)
        .map(|(entity, dock, _)| (entity, format!("{:?}", dock.0)))
        .collect();
    docks
        .into_iter()
        .filter_map(|(entity, name)| rect_of(world, entity).map(|rect| (name, rect)))
        .collect()
}

fn row_count(world: &mut World) -> usize {
    world
        .query::<&crate::assets::AssetRowNode>()
        .iter(world)
        .count()
}

pub(crate) fn probe_assets(world: &mut World) {
    let frame = {
        let mut probe = world.resource_mut::<AssetsProbe>();
        probe.frame += 1;
        probe.frame
    };
    match frame {
        // Boot: past the menu and into the editor. Panel toggles stand down
        // while the editor is inactive, so nothing below works without this.
        60 => {
            crate::probe_user::tap_named(world, KeyCode::Enter, bevy::input::keyboard::Key::Enter)
        }
        120 => invoke(world, "core.toggle-editor"),
        150 => {
            let entries = world.resource::<ModelLibrary>().entries.len();
            check(
                world,
                entries > 0,
                &format!("the startup import found assets ({entries})"),
            );
        }
        // The panel starts closed — it is the first tenant of the bottom dock
        // and an open one costs viewport height every session.
        160 => {
            let open = world
                .resource::<PanelStates>()
                .open(&editor_api::prelude::PanelId::new_static("assets"));
            check(world, !open, "the assets panel starts closed");
        }
        170 => invoke(world, "panel.toggle.assets"),
        190 => {
            let open = world
                .resource::<PanelStates>()
                .open(&editor_api::prelude::PanelId::new_static("assets"));
            check(world, open, "the toggle opened the panel");
            let bottom = dock_rects(world)
                .into_iter()
                .find(|(name, _)| name == "Bottom");
            match bottom {
                Some((_, rect)) => {
                    let height = rect.height();
                    let wanted = world.resource::<EditorSettings>().ui.dock_bottom_height;
                    check(
                        world,
                        (height - wanted).abs() < 2.0,
                        &format!("the bottom dock is {height:.0}px, settings say {wanted:.0}"),
                    );
                }
                None => check(world, false, "the bottom dock materialized"),
            }
        }
        // A row per asset, plus a group header per directory.
        210 => {
            let entries = world.resource::<ModelLibrary>().entries.len();
            let rows = row_count(world);
            check(
                world,
                rows > entries,
                &format!("{rows} rows for {entries} assets (headers included)"),
            );
        }
        // The numbers the Process stage measured, reaching a designer for the
        // first time.
        220 => {
            let sized = world
                .query::<&bevy::ui::widget::Text>()
                .iter(world)
                .filter(|text| text.0.contains('\u{d7}') && text.0.ends_with(" m"))
                .count();
            check(
                world,
                sized > 0,
                &format!("a model row shows its measured size ({sized} rows)"),
            );
        }
        // GEOMETRY: the timeline must not draw over any dock — including the
        // bottom one this panel just created.
        240 => invoke(world, "timeline.toggle"),
        270 => {
            let timeline = world
                .query_filtered::<Entity, With<crate::timeline_panel::TimelinePanel>>()
                .iter(world)
                .next();
            match timeline.and_then(|e| rect_of(world, e)) {
                Some(timeline_rect) => {
                    let overlaps: Vec<String> = dock_rects(world)
                        .into_iter()
                        .filter(|(_, dock)| !timeline_rect.intersect(*dock).is_empty())
                        .map(|(name, _)| name)
                        .collect();
                    check(
                        world,
                        overlaps.is_empty(),
                        &format!("the timeline clears every dock (overlaps: {overlaps:?})"),
                    );
                }
                None => check(world, false, "the timeline has a rect to check"),
            }
        }
        290 => invoke(world, "timeline.toggle"),
        // A source that fails validation is MARKED, not silently listed.
        320 => {
            let root = world.resource::<ModelLibrary>().fs_root.clone();
            let _ = std::fs::write(root.join("models").join("probe-empty.glb"), b"");
            invoke(world, "asset.import");
        }
        350 => {
            let (found, duplicates, scanned) = {
                let problems = world.resource::<AssetProblems>();
                let found = problems.problems.iter().any(|p| {
                    p.path.contains("probe-empty.glb")
                        && p.severity == Severity::Error
                        && matches!(&p.source, ProblemSource::Validator(id) if id.as_str() == "asset.nonempty")
                });
                let duplicates = problems
                    .problems
                    .iter()
                    .filter(|p| {
                        p.path.contains("probe-empty.glb")
                            && matches!(&p.source, ProblemSource::Validator(id) if id.as_str() == "gltf.parse")
                    })
                    .count();
                (found, duplicates, problems.scanned)
            };
            check(
                world,
                found,
                "the empty source is an Error naming its validator",
            );
            check(
                world,
                duplicates <= 1,
                &format!("the parse failure is reported once, not twice ({duplicates})"),
            );
            check(
                world,
                scanned > 0,
                &format!("the walk reports its denominator ({scanned} files)"),
            );
        }
        // ...and it reaches the surfaces a designer actually looks at.
        380 => invoke(world, "level.problems"),
        410 => {
            let has_assets_section = world
                .query::<&bevy::ui::widget::Text>()
                .iter(world)
                .any(|text| text.0 == "ASSETS");
            check(
                world,
                has_assets_section,
                "the problems panel grew an ASSETS section",
            );
            let statusbar = world
                .query::<&bevy::ui::widget::Text>()
                .iter(world)
                .any(|text| text.0.contains("assets \u{d7}"));
            check(world, statusbar, "the status bar carries the assets count");
            shot(world, "01-asset-browser");
        }
        440 => {
            let root = world.resource::<ModelLibrary>().fs_root.clone();
            let _ = std::fs::remove_file(root.join("models").join("probe-empty.glb"));
            let _ = std::fs::remove_file(root.join("models").join("probe-empty.glb.import.ron"));
            let failures = world.resource::<AssetsProbe>().failures.clone();
            if failures.is_empty() {
                info!("ASSETS-PROBE PASS: the asset browser end-to-end");
                world.write_message(AppExit::Success);
            } else {
                error!("ASSETS-PROBE FAILED: {failures:?}");
                world.write_message(AppExit::error());
            }
        }
        _ => {}
    }
}
