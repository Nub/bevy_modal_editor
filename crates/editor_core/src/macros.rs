//! Macros (M2, B11): record the action stream, replay it through the exact same
//! dispatch path (RFC §4 — this is what the no-side-door rule buys). `q` toggles
//! recording, `shift+2` (@) replays; a replay's edits merge into ONE undo entry.

use bevy::prelude::*;
use editor_api::prelude::*;

use crate::edits::MergeFrameEntries;

#[derive(Resource, Default)]
pub struct MacroState {
    /// `Some` while recording (the actions captured so far).
    pub recording: Option<Vec<ActionId>>,
    /// The last finished recording — what replay plays.
    pub last: Vec<ActionId>,
}

/// Actions the recorder itself must never capture.
fn is_control(action: &ActionId) -> bool {
    matches!(action.as_str(), "core.macro-record" | "core.macro-replay" | "core.toggle-editor")
}

/// Replay queue between the reader half and the writer half (a reader and writer on
/// the same message type in one system would conflict).
#[derive(Resource, Default)]
pub(crate) struct PendingReplay(Vec<ActionId>);

/// FIRST in the Tools chain: captures this frame's actions; queues replay emission.
pub(crate) fn record_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut state: ResMut<MacroState>,
    mut pending: ResMut<PendingReplay>,
) {
    let mut replay_requested = false;
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "core.macro-record" => {
                if let Some(recorded) = state.recording.take() {
                    if !recorded.is_empty() {
                        state.last = recorded;
                    }
                } else {
                    state.recording = Some(Vec::new());
                }
            }
            "core.macro-replay" => replay_requested = true,
            _ => {
                if invoked.source != InvocationSource::Macro && !is_control(&invoked.action) {
                    if let Some(recording) = &mut state.recording {
                        recording.push(invoked.action.clone());
                    }
                }
            }
        }
    }
    if replay_requested && state.recording.is_none() && !state.last.is_empty() {
        pending.0.extend(state.last.iter().cloned());
    }
}

/// Immediately after: emit the queued replay through the normal dispatch path (all
/// downstream handlers run later this same frame), and arm the history merge so the
/// whole replay lands as one undo entry.
pub(crate) fn emit_replay(
    mut pending: ResMut<PendingReplay>,
    mut writer: MessageWriter<ActionInvoked>,
    mut merge: ResMut<MergeFrameEntries>,
) {
    if pending.0.is_empty() {
        return;
    }
    merge.0 = true;
    for action in pending.0.drain(..) {
        writer.write(ActionInvoked { action, args: None, source: InvocationSource::Macro });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edits::{History, HistoryRequests};
    use crate::resolver::EditorState;
    use crate::EditorCorePlugin;

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component)]
    struct Marker;

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("test", "Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Marker>()
                .action(ActionDef::new("test.spawn-thing", "Spawn Thing").edit());
        }
    }

    fn spawn_handler(mut reader: MessageReader<ActionInvoked>, mut edits: EditScope) {
        for invoked in reader.read() {
            if invoked.action.as_str() == "test.spawn-thing" {
                edits
                    .transaction("Spawn thing")
                    .spawn(
                        SceneId::random(),
                        vec![Box::new(Marker).into_partial_reflect()],
                    )
                    .commit();
            }
        }
    }

    fn test_app() -> App {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<ButtonInput<KeyCode>>();
        app.add_systems(
            Update,
            spawn_handler
                .after(super::emit_replay)
                .in_set(crate::EditorSet::Tools),
        );
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;
        app
    }

    fn invoke(app: &mut App, action: &str) {
        app.world_mut().write_message(ActionInvoked {
            action: ActionId::new(action.to_string()),
            args: None,
            source: InvocationSource::Test,
        });
        app.update();
    }

    fn marker_count(app: &mut App) -> usize {
        let world = app.world_mut();
        world.query_filtered::<(), With<Marker>>().iter(world).count()
    }

    // B11: record -> replay reproduces the edits as ONE coalesced undo entry.
    #[test]
    fn record_and_replay_merges_history() {
        let mut app = test_app();

        invoke(&mut app, "core.macro-record");
        invoke(&mut app, "test.spawn-thing");
        invoke(&mut app, "test.spawn-thing");
        invoke(&mut app, "core.macro-record"); // stop
        assert_eq!(marker_count(&mut app), 2);
        assert_eq!(app.world().resource::<History>().undo_depth(), 2);
        assert_eq!(app.world().resource::<MacroState>().last.len(), 2);

        invoke(&mut app, "core.macro-replay");
        app.update();
        assert_eq!(marker_count(&mut app), 4, "replay reproduces the edits");
        assert_eq!(
            app.world().resource::<History>().undo_depth(),
            3,
            "whole replay = ONE history entry"
        );

        app.world_mut().resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        assert_eq!(marker_count(&mut app), 2, "one undo removes the whole replay");
    }
}
