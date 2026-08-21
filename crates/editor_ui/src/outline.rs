//! Selection outlines: v1's JFA silhouette outliner (`bevy_outliner`, carried through
//! the port gate and reworked onto 0.19 render schedules — ledger #8) driven by the
//! kernel's `Selected` state. Replaces the interim AABB gizmo.

use bevy::prelude::*;
use bevy_outliner::prelude::*;
use editor_core::prelude::*;

/// Outline styling from settings (JFA wants linear color).
fn outline(settings: &EditorSettings, locked: bool) -> MeshOutline {
    let [r, g, b, a] = if locked {
        settings.viewport.locked_outline_color
    } else {
        settings.viewport.outline_color
    };
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
///
/// The outline also CARRIES the lock: a locked object silhouettes in the warn
/// tone instead of the selection blue, so "selected but it won't move" is
/// visible in the viewport, where you are looking when you try to move it —
/// not only in the hierarchy.
pub(crate) fn sync_selection_outlines(
    state: Res<EditorState>,
    settings: Res<EditorSettings>,
    mut commands: Commands,
    selected: Query<(Entity, Has<editor_core::lock::Locked>), With<Selected>>,
    hidden: Res<editor_core::hide::Hidden>,
    scene_ids: Query<&editor_api::prelude::SceneId>,
    parents: Query<&ChildOf>,
    children_query: Query<&Children>,
    meshes: Query<(), With<Mesh3d>>,
    outlined: Query<(Entity, &MeshOutline)>,
) {
    if !state.active {
        for (entity, _) in &outlined {
            commands.entity(entity).remove::<MeshOutline>();
        }
        return;
    }
    let mut desired: bevy::platform::collections::HashMap<Entity, bool> = Default::default();
    for (root, locked) in &selected {
        // The JFA silhouette is an UNPARENTED root at the source's world
        // transform, so `Visibility::Hidden` on the object does not reach it:
        // a hidden object selected from its hierarchy row would draw an
        // outline around empty space.
        if editor_core::hide::is_hidden(root, &hidden, &scene_ids, &parents) {
            continue;
        }
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if meshes.contains(entity) {
                // A locked ANCESTOR locks what is under it: the subtree cannot
                // move independently, so it must not read as free.
                *desired.entry(entity).or_insert(locked) |= locked;
            }
            if let Ok(children) = children_query.get(entity) {
                stack.extend(children.iter());
            }
        }
    }
    for (entity, current) in &outlined {
        match desired.get(&entity) {
            None => {
                commands.entity(entity).remove::<MeshOutline>();
            }
            // Already outlined the right way — leave it alone rather than
            // rewriting the component every frame and re-extracting it.
            Some(&locked) if current.color == outline(&settings, locked).color => {
                desired.remove(&entity);
            }
            Some(_) => {}
        }
    }
    for (entity, locked) in desired {
        commands.entity(entity).insert(outline(&settings, locked));
    }
}
