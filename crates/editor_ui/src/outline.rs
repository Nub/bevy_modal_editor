//! Selection outlines: v1's JFA silhouette outliner (`bevy_outliner`, carried through
//! the port gate and reworked onto 0.19 render schedules — ledger #8) driven by the
//! kernel's `Selected` state. Replaces the interim AABB gizmo.

use bevy::prelude::*;
use bevy_outliner::prelude::*;
use editor_core::prelude::*;

/// Outline styling from settings (JFA wants linear color).
fn outline(settings: &EditorSettings) -> MeshOutline {
    let [r, g, b, a] = settings.viewport.outline_color;
    MeshOutline::new(LinearRgba::new(r, g, b, a), settings.viewport.outline_width)
}

/// The editor's viewport camera renders outlines. The silhouette camera the outliner
/// spawns is itself a `Camera3d` — never give IT `OutlineSettings` or it would
/// recurse into silhouetting its own silhouettes.
pub(crate) fn ensure_outline_camera(
    mut commands: Commands,
    cameras: Query<
        Entity,
        (
            With<Camera3d>,
            Without<OutlineSettings>,
            Without<SilhouetteCamera>,
        ),
    >,
) {
    for entity in &cameras {
        commands.entity(entity).insert(OutlineSettings::default());
    }
}

/// `Selected` <-> `MeshOutline` sync. Outlines are editor-session visuals only:
/// deactivating the editor (F12, play) strips them all so the game view is clean.
pub(crate) fn sync_selection_outlines(
    state: Res<EditorState>,
    settings: Res<EditorSettings>,
    mut commands: Commands,
    missing: Query<Entity, (With<Selected>, With<Mesh3d>, Without<MeshOutline>)>,
    stale: Query<Entity, (With<MeshOutline>, Without<Selected>)>,
    all_outlined: Query<Entity, With<MeshOutline>>,
) {
    if !state.active {
        for entity in &all_outlined {
            commands.entity(entity).remove::<MeshOutline>();
        }
        return;
    }
    for entity in &missing {
        commands.entity(entity).insert(outline(&settings));
    }
    for entity in &stale {
        commands.entity(entity).remove::<MeshOutline>();
    }
}
