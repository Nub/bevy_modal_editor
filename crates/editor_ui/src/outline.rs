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
///
/// The outline covers the selection's whole VISUAL SUBTREE: imported models
/// (`MeshRef`) and prefab instance roots carry no `Mesh3d` themselves — their
/// meshes live on (derived) descendants, which must silhouette as one thing.
/// Recomputed every frame: gltf content spawns async, so a selected model's
/// meshes may not exist yet on the frame selection changes.
pub(crate) fn sync_selection_outlines(
    state: Res<EditorState>,
    settings: Res<EditorSettings>,
    mut commands: Commands,
    selected: Query<Entity, With<Selected>>,
    children_query: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
    outlined: Query<Entity, With<MeshOutline>>,
) {
    if !state.active {
        for entity in &outlined {
            commands.entity(entity).remove::<MeshOutline>();
        }
        return;
    }
    let mut desired: bevy::platform::collections::HashSet<Entity> = Default::default();
    for root in &selected {
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if meshes.contains(entity) {
                desired.insert(entity);
            }
            if let Ok(children) = children_query.get(entity) {
                stack.extend(children.iter());
            }
        }
    }
    for entity in &outlined {
        if !desired.contains(&entity) {
            commands.entity(entity).remove::<MeshOutline>();
        } else {
            desired.remove(&entity);
        }
    }
    for entity in desired {
        commands.entity(entity).insert(outline(&settings));
    }
}
