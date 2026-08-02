//! Selection clipboard (keymap doc: "the selection is the text object") — `d`
//! deletes/cuts the selection into the clipboard, `y` yanks it, `p` pastes copies.
//! Every mutation is an `EditScope` transaction: one undo entry per cut, one per
//! paste, restored exactly.

use bevy::prelude::*;
use editor_api::prelude::*;

use crate::edits::EditorComponents;
use crate::resolver::EditorState;
use crate::selection::{Selected, SelectionChanged};

/// Captured component sets (registered components only — the same capture set as
/// despawn inverses and serialization).
#[derive(Resource, Default)]
pub struct EditorClipboard {
    pub entries: Vec<Vec<Box<dyn PartialReflect>>>,
}

#[derive(Resource, Default)]
pub(crate) struct ClipboardRequests {
    cut: bool,
    yank: bool,
    paste: bool,
}

/// New ids from the last paste — selected once the spawns have applied.
#[derive(Resource, Default)]
pub(crate) struct PendingPasteSelect(Vec<SceneId>);

pub(crate) fn collect_clipboard_actions(
    mut reader: MessageReader<ActionInvoked>,
    state: Res<EditorState>,
    mut requests: ResMut<ClipboardRequests>,
) {
    if !state.active {
        return;
    }
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "select.delete" => requests.cut = true,
            "select.yank" => requests.yank = true,
            "select.paste" => requests.paste = true,
            _ => {}
        }
    }
}

fn capture_selected(world: &mut World) -> Vec<(SceneId, Vec<Box<dyn PartialReflect>>)> {
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    let components = world.resource::<EditorComponents>().types.clone();
    let mut selected: Vec<(Entity, SceneId)> = {
        let mut query = world.query_filtered::<(Entity, &SceneId), With<Selected>>();
        query.iter(world).map(|(e, id)| (e, *id)).collect()
    };
    selected.sort_by_key(|(_, id)| id.0);
    selected
        .into_iter()
        .map(|(entity, id)| {
            let mut captured = Vec::new();
            for reg in &components {
                let Some(registration) = registry.get(reg.type_id) else { continue };
                let Some(reflect_component) =
                    registration.data::<bevy::ecs::reflect::ReflectComponent>()
                else {
                    continue;
                };
                if let Some(value) = reflect_component.reflect(world.entity(entity)) {
                    captured.push(value.as_partial_reflect().to_dynamic());
                }
            }
            (id, captured)
        })
        .collect()
}

pub(crate) fn perform_clipboard(world: &mut World) {
    let requests = std::mem::take(&mut *world.resource_mut::<ClipboardRequests>());
    if !requests.cut && !requests.yank && !requests.paste {
        return;
    }

    if requests.yank || requests.cut {
        let captured = capture_selected(world);
        if !captured.is_empty() {
            world.resource_mut::<EditorClipboard>().entries =
                captured.iter().map(|(_, c)| c.iter().map(|v| v.to_dynamic()).collect()).collect();
        }
        if requests.cut {
            let ids: Vec<SceneId> = captured.iter().map(|(id, _)| *id).collect();
            if !ids.is_empty() {
                let ops = ids.into_iter().map(|id| Op::Despawn { id }).collect::<Vec<_>>();
                let label = format!("Delete {}", ops.len());
                world
                    .resource_mut::<EditQueue>()
                    .0
                    .push(Transaction { label, gesture: None, ops });
            }
        }
    }

    if requests.paste {
        let entries: Vec<Vec<Box<dyn PartialReflect>>> = {
            let clipboard = world.resource::<EditorClipboard>();
            clipboard
                .entries
                .iter()
                .map(|entry| entry.iter().map(|v| v.to_dynamic()).collect())
                .collect()
        };
        if !entries.is_empty() {
            let mut new_ids = Vec::new();
            let ops = entries
                .into_iter()
                .map(|components| {
                    let id = SceneId::random();
                    new_ids.push(id);
                    Op::Spawn { id, components }
                })
                .collect::<Vec<_>>();
            let label = format!("Paste {}", ops.len());
            world.resource_mut::<EditQueue>().0.push(Transaction { label, gesture: None, ops });
            world.resource_mut::<PendingPasteSelect>().0 = new_ids;
        }
    }
}

/// After the paste transaction applies (Mutate), select what was pasted.
pub(crate) fn select_pasted(
    mut pending: ResMut<PendingPasteSelect>,
    index: Res<SceneIndex>,
    previous: Query<Entity, With<Selected>>,
    mut changed: MessageWriter<SelectionChanged>,
    mut commands: Commands,
) {
    if pending.0.is_empty() {
        return;
    }
    let resolved: Vec<Entity> = pending.0.iter().filter_map(|id| index.get(id)).collect();
    if resolved.is_empty() {
        return; // spawns not applied yet — retry next frame
    }
    pending.0.clear();
    for entity in &previous {
        commands.entity(entity).remove::<Selected>();
    }
    for entity in resolved {
        commands.entity(entity).insert(Selected);
    }
    changed.write(SelectionChanged);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edits::{History, HistoryRequests};
    use crate::EditorCorePlugin;

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component)]
    struct Payload(f32);

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("clip-test", "Clip Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Payload>();
        }
    }

    fn invoke(app: &mut App, action: &str) {
        app.world_mut().write_message(ActionInvoked {
            action: ActionId::new(action.to_string()),
            args: None,
            source: InvocationSource::Test,
        });
        app.update();
    }

    fn payload_values(app: &mut App) -> Vec<f32> {
        let world = app.world_mut();
        let mut values: Vec<f32> =
            world.query::<&Payload>().iter(world).map(|p| p.0).collect();
        values.sort_by(f32::total_cmp);
        values
    }

    // d cuts (one undoable delete), p pastes copies (one undoable spawn, selected).
    #[test]
    fn cut_paste_round_trip() {
        let mut app = App::new();
        app.add_plugins(EditorCorePlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<bevy::input::ButtonInput<KeyCode>>();
        app.finish();
        app.update();
        app.world_mut().resource_mut::<EditorState>().active = true;

        for value in [1.0_f32, 2.0] {
            app.world_mut().resource_mut::<EditQueue>().0.push(Transaction {
                label: "spawn".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: SceneId::random(),
                    components: vec![Box::new(Payload(value)).into_partial_reflect()],
                }],
            });
        }
        app.update();
        let world = app.world_mut();
        let entities: Vec<Entity> =
            world.query_filtered::<Entity, With<SceneId>>().iter(world).collect();
        for entity in entities {
            world.entity_mut(entity).insert(Selected);
        }

        invoke(&mut app, "select.delete");
        assert_eq!(payload_values(&mut app), Vec::<f32>::new(), "cut removed both");

        // Undo the cut restores them.
        app.world_mut().resource_mut::<HistoryRequests>().undo = 1;
        app.update();
        assert_eq!(payload_values(&mut app), vec![1.0, 2.0]);

        // Paste adds copies and selects them.
        let depth = app.world().resource::<History>().undo_depth();
        invoke(&mut app, "select.paste");
        app.update();
        assert_eq!(payload_values(&mut app), vec![1.0, 1.0, 2.0, 2.0]);
        assert_eq!(app.world().resource::<History>().undo_depth(), depth + 1);
        let world = app.world_mut();
        let selected = world.query_filtered::<(), With<Selected>>().iter(world).count();
        assert_eq!(selected, 2, "pasted entities are the new selection");
    }
}
