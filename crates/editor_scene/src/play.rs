//! Play / pause / reset (M2, B10): Play snapshots the scene through the SAME
//! `SceneSnapshot` choke point as save/load, hands input to the game; Pause freezes
//! virtual time; Reset restores the snapshot EXACTLY — selection, camera, dirty flag,
//! and undo history intact (SceneId-targeted history survives the respawn).

use crate::{apply_scene, capture_scene, SceneDirty, SceneIoFeedback, SceneSnapshot};
use bevy::prelude::*;
use editor_core::prelude::*;

pub struct PlaySession {
    snapshot: SceneSnapshot,
    selected: Vec<SceneId>,
    camera: Option<Transform>,
    dirty: bool,
}

/// `Some` while a play session is running (scene state is ephemeral until reset).
#[derive(Resource, Default)]
pub struct PlayState(pub Option<PlaySession>);

#[derive(Resource, Default)]
pub struct PlayRequests {
    play: bool,
    pause: bool,
    reset: bool,
}

pub struct PlayFeature;

impl EditorFeature for PlayFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("play", "Play In Editor")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.action(
            ActionDef::new("editor.play", "Play")
                .describe("Snapshot the scene and hand control to the game")
                .context("normal")
                .bind("f5"),
        )
        // Pause/Reset live in the GLOBAL context: they must work while playing.
        .action(
            ActionDef::new("editor.pause", "Pause / Resume")
                .describe("Freeze or resume game time")
                .bind("f6"),
        )
        .action(
            ActionDef::new("editor.reset", "Reset Play Session")
                .describe("Restore the exact pre-play scene, selection, and camera")
                .bind("f7"),
        );
    }
}

pub(crate) fn collect_play_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut requests: ResMut<PlayRequests>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "editor.play" => requests.play = true,
            "editor.pause" => requests.pause = true,
            "editor.reset" => requests.reset = true,
            _ => {}
        }
    }
}

pub(crate) fn perform_play(world: &mut World) {
    let requests = std::mem::take(&mut *world.resource_mut::<PlayRequests>());
    if !requests.play && !requests.pause && !requests.reset {
        return;
    }

    if requests.play && world.resource::<PlayState>().0.is_none() {
        let snapshot = capture_scene(world);
        let selected: Vec<SceneId> = world
            .query_filtered::<&SceneId, With<Selected>>()
            .iter(world)
            .copied()
            .collect();
        let camera = world
            .query::<(&Camera, &Transform)>()
            .iter(world)
            .find(|(c, _)| c.is_active)
            .map(|(_, t)| *t);
        let dirty = world.resource::<SceneDirty>().0;
        world.resource_mut::<PlayState>().0 =
            Some(PlaySession { snapshot, selected, camera, dirty });
        world.resource_mut::<EditorState>().active = false;
        world.write_message(SceneIoFeedback {
            message: "playing — F6 pause · F7 reset".into(),
            success: true,
        });
    }

    if requests.pause {
        if let Some(mut time) = world.get_resource_mut::<Time<Virtual>>() {
            if time.is_paused() {
                time.unpause();
            } else {
                time.pause();
            }
        }
    }

    if requests.reset {
        if let Some(session) = world.resource_mut::<PlayState>().0.take() {
            // Exact restore through the one choke point; history is PRESERVED.
            apply_scene(world, &session.snapshot, false);

            // Selection back onto the (re-spawned) entities.
            for id in &session.selected {
                if let Some(entity) = world.resource::<SceneIndex>().get(id) {
                    world.entity_mut(entity).insert(Selected);
                }
            }
            // Camera exactly where the editor left it.
            if let Some(saved) = session.camera {
                let camera = world
                    .query::<(&Camera, &mut Transform)>()
                    .iter_mut(world)
                    .find(|(c, _)| c.is_active)
                    .map(|(_, t)| t);
                if let Some(mut transform) = camera {
                    *transform = saved;
                }
            }
            world.resource_mut::<SceneDirty>().0 = session.dirty;
            if let Some(mut time) = world.get_resource_mut::<Time<Virtual>>() {
                time.unpause();
            }
            world.resource_mut::<EditorState>().active = true;
            world.write_message(SceneIoFeedback {
                message: "reset to pre-play state".into(),
                success: true,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests_support::*;

    // B10: play -> runtime chaos -> reset restores scene, selection, history exactly.
    #[test]
    fn play_reset_round_trip() {
        let mut app = scene_test_app();
        let (a, b) = spawn_test_scene(&mut app);

        // Select `a`, note history + serialized state.
        let entity_a = app.world().resource::<SceneIndex>().get(&a).unwrap();
        app.world_mut().entity_mut(entity_a).insert(Selected);
        let depth_before = app.world().resource::<editor_core::prelude::History>().undo_depth();
        let state_before = scene_ron(&mut app);

        invoke(&mut app, "editor.play");
        assert!(!app.world().resource::<EditorState>().active, "game owns input");
        assert!(app.world().resource::<PlayState>().0.is_some());

        // Simulate gameplay chaos: mutate a transform directly, despawn an entity.
        {
            let world = app.world_mut();
            let entity_a = world.resource::<SceneIndex>().get(&a).unwrap();
            world.get_mut::<Transform>(entity_a).unwrap().translation = Vec3::splat(99.0);
            let entity_b = world.resource::<SceneIndex>().get(&b).unwrap();
            world.entity_mut(entity_b).despawn();
        }
        assert_ne!(scene_ron(&mut app), state_before);

        invoke(&mut app, "editor.reset");
        assert_eq!(scene_ron(&mut app), state_before, "exact scene restore");
        assert!(app.world().resource::<EditorState>().active, "editor input back");
        assert_eq!(
            app.world().resource::<editor_core::prelude::History>().undo_depth(),
            depth_before,
            "history preserved across reset (B10)"
        );
        let world = app.world_mut();
        let entity_a = world.resource::<SceneIndex>().get(&a).unwrap();
        assert!(world.get::<Selected>(entity_a).is_some(), "selection restored");
    }
}
