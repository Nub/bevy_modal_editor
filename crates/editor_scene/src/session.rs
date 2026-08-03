//! Fast-relaunch session continuity (M3-C8 fallback, spike: spikes/hot-reload).
//!
//! `editor.reload` saves the scene, writes a session sidecar (selection, camera,
//! editor-active), spawns a fresh copy of the current binary, and exits. On boot,
//! a FRESH sidecar (younger than 60s — stale ones are ignored, never replayed
//! surprisingly) is consumed by the game shell to skip the menu, reload the
//! scene, and restore the editing context. Combined with `cargo watch`, this is
//! the tweak→rebuild→same-scene loop until real hot reload lands upstream.

use crate::{SceneFile, SceneIoFeedback};
use bevy::prelude::*;
use editor_core::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const SESSION_PATH: &str = ".editor-session.ron";

#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(default)]
pub struct EditorSession {
    pub scene_path: PathBuf,
    pub selection: Vec<SceneId>,
    pub camera: Option<[f32; 16]>,
    pub editor_active: bool,
    /// Unix seconds at write time (freshness gate).
    pub written_at: u64,
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Consume a FRESH sidecar (young enough to be a deliberate relaunch).
pub fn take_session() -> Option<EditorSession> {
    let text = std::fs::read_to_string(SESSION_PATH).ok()?;
    let _ = std::fs::remove_file(SESSION_PATH);
    let session: EditorSession = ron::from_str(&text).ok()?;
    (now_unix().saturating_sub(session.written_at) < 60).then_some(session)
}

pub(crate) struct ReloadFeature;

impl EditorFeature for ReloadFeature {
    fn manifest(&self) -> FeatureManifest {
        FeatureManifest::new("reload", "Fast Relaunch")
    }
    fn register(&self, reg: &mut FeatureRegistry) {
        reg.action(
            ActionDef::new("editor.reload", "Reload Editor (fast relaunch)")
                .describe("Save everything and restart into the freshly built binary")
                .bind("ctrl+shift+r"),
        );
    }
}

#[derive(Resource, Default)]
pub(crate) struct ReloadRequested(pub bool);

pub(crate) fn collect_reload_action(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<EditorState>,
    mut requested: ResMut<ReloadRequested>,
) {
    for invoked in reader.read() {
        if invoked.action.as_str() == "editor.reload" && state.active {
            requested.0 = true;
        }
    }
}

/// Exclusive: save scene + sidecar, spawn the (rebuilt) binary, exit.
pub(crate) fn perform_reload(world: &mut World) {
    if !std::mem::take(&mut world.resource_mut::<ReloadRequested>().0) {
        return;
    }
    if world.resource::<crate::SceneIoLock>().0 {
        world.write_message(SceneIoFeedback {
            message: "close the open prefab before reloading".into(),
            success: false,
        });
        return;
    }
    let scene_path = world.resource::<SceneFile>().0.clone();
    if let Err(e) = crate::save_scene_file(world, &scene_path) {
        world.write_message(SceneIoFeedback {
            message: format!("reload aborted: scene save failed: {e}"),
            success: false,
        });
        return;
    }
    let selection: Vec<SceneId> = {
        let mut query = world.query_filtered::<&SceneId, With<Selected>>();
        query.iter(world).copied().collect()
    };
    let camera = {
        let mut query = world
            .query::<(&Camera, &Transform, Option<&bevy::camera::RenderTarget>)>();
        query
            .iter(world)
            .find(|(c, _, target)| is_viewport_camera(c, *target))
            .map(|(_, t, _)| t.to_matrix().to_cols_array())
    };
    let session = EditorSession {
        scene_path,
        selection,
        camera,
        editor_active: world.resource::<EditorState>().active,
        written_at: now_unix(),
    };
    let Ok(text) = ron::to_string(&session) else { return };
    if std::fs::write(SESSION_PATH, text).is_err() {
        return;
    }
    let Ok(current) = std::env::current_exe() else { return };
    match std::process::Command::new(current).spawn() {
        Ok(_) => {
            world.write_message(bevy::app::AppExit::Success);
        }
        Err(e) => {
            let _ = std::fs::remove_file(SESSION_PATH);
            world.write_message(SceneIoFeedback {
                message: format!("reload failed to spawn: {e}"),
                success: false,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Freshness gate: stale sidecars are never replayed.
    #[test]
    fn stale_sessions_are_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let old = std::env::current_dir().unwrap();
        std::env::set_current_dir(dir.path()).unwrap();

        let fresh = EditorSession { written_at: now_unix(), ..Default::default() };
        std::fs::write(SESSION_PATH, ron::to_string(&fresh).unwrap()).unwrap();
        assert!(take_session().is_some(), "fresh session consumed");
        assert!(!std::path::Path::new(SESSION_PATH).exists(), "sidecar removed");

        let stale = EditorSession { written_at: now_unix() - 3600, ..Default::default() };
        std::fs::write(SESSION_PATH, ron::to_string(&stale).unwrap()).unwrap();
        assert!(take_session().is_none(), "stale session ignored");

        std::env::set_current_dir(old).unwrap();
    }
}
