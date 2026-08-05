//! Override semantics (M4-D5): overrides are DERIVED, not intercepted — a
//! per-field reflection diff between each stamped entity's current state and
//! its template, synced into the instance root's `PrefabOverrides` whenever
//! stamped entities are edited. Consequences, all by construction:
//! - EVERY edit path (gesture drag, inspector, clipboard paste) becomes an
//!   override — there is no second editing pipeline to keep honest;
//! - undo just works: state is undoable, overrides derive from state;
//! - stamping re-applies patches OVER the template, so a template update
//!   propagates to every instance while overridden fields stay put.
//!
//! Verbs: `prefab.revert-overrides` clears the deltas (undoable Set) and
//! re-stamps; `prefab.apply-to-prefab` folds this instance's deltas INTO the
//! library template (bumping its generation → every instance re-stamps).

use crate::{OverridePatch, PrefabInstance, PrefabLibrary, PrefabOverrides};
use bevy::prelude::*;
use bevy::reflect::serde::{TypedReflectDeserializer, TypedReflectSerializer};
use bevy::reflect::{ParsedPath, PartialReflect, ReflectPath, ReflectRef, TypeRegistry};
use editor_core::edits::EditorComponents;
use editor_core::prelude::*;
use serde::de::DeserializeSeed;

/// Maps a stamped entity back to its template record (never serialized).
#[derive(Component, Clone, Copy)]
pub struct StampedFrom {
    pub instance_root: SceneId,
    pub template_id: SceneId,
}

/// Recursive per-field leaf diff: paths into `current` whose values differ from
/// `template`, values RON-serialized. Component-granular fallbacks when a leaf
/// can't be compared.
fn diff_fields(
    registry: &TypeRegistry,
    prefix: &str,
    template: &dyn PartialReflect,
    current: &dyn PartialReflect,
    out: &mut Vec<(String, String)>,
) {
    match (template.reflect_ref(), current.reflect_ref()) {
        (ReflectRef::Struct(t), ReflectRef::Struct(c)) => {
            for i in 0..c.field_len() {
                let name = c.name_at(i).unwrap_or_default();
                let path = if prefix.is_empty() {
                    name.to_string()
                } else {
                    format!("{prefix}.{name}")
                };
                if let (Some(tf), Some(cf)) = (t.field(name), c.field_at(i)) {
                    diff_fields(registry, &path, tf, cf, out);
                }
            }
        }
        _ => {
            let equal = template.reflect_partial_eq(current).unwrap_or(false);
            if !equal
                && let Ok(value) =
                    ron::ser::to_string(&TypedReflectSerializer::new(current, registry))
            {
                out.push((prefix.to_string(), value));
            }
        }
    }
}

/// Apply one RON patch onto a live component value (shared by stamping and
/// apply-to-prefab folding).
pub(crate) fn apply_patch_value(
    registry: &TypeRegistry,
    component: &mut dyn PartialReflect,
    path: &str,
    value_ron: &str,
) -> bool {
    let Ok(parsed) = ParsedPath::parse(path) else {
        return false;
    };
    let Ok(element) = parsed.reflect_element_mut(component) else {
        return false;
    };
    let Some(info) = element.get_represented_type_info() else {
        return false;
    };
    let Some(registration) = registry.get(info.type_id()) else {
        return false;
    };
    let mut deserializer = match ron::Deserializer::from_str(value_ron) {
        Ok(d) => d,
        Err(_) => return false,
    };
    let Ok(new_value) =
        TypedReflectDeserializer::new(registration, registry).deserialize(&mut deserializer)
    else {
        return false;
    };
    element.apply(new_value.as_ref());
    true
}

/// Non-stealing message cursor (other systems read `Edited` too).
#[derive(Resource, Default)]
pub struct OverrideCursor(bevy::ecs::message::MessageCursor<Edited>);

/// Derive overrides: whenever an edit touches stamped entities, re-diff their
/// instances and write the deltas into the roots' `PrefabOverrides` (direct
/// write — derived state, like the SceneIndex; the USER-level undo lives in the
/// component edits the diff derives from).
pub fn sync_overrides(world: &mut World) {
    let touched: Vec<SceneId> = world.resource_scope(|world, mut cursor: Mut<OverrideCursor>| {
        let messages = world.resource::<bevy::ecs::message::Messages<Edited>>();
        cursor
            .0
            .read(messages)
            .flat_map(|e| e.targets.clone())
            .collect()
    });
    if touched.is_empty() {
        return;
    }
    // Which instance roots own touched STAMPED entities?
    let open_root = world
        .resource::<crate::open_mode::OpenInstance>()
        .0
        .as_ref()
        .map(|o| o.root);
    let mut roots: Vec<SceneId> = touched
        .iter()
        .filter_map(|id| world.resource::<SceneIndex>().get(id))
        .filter_map(|entity| world.get::<StampedFrom>(entity))
        .map(|s| s.instance_root)
        .filter(|root| Some(*root) != open_root)
        .collect();
    roots.sort_by_key(|id| id.0);
    roots.dedup();
    if roots.is_empty() {
        return;
    }
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    let components = world.resource::<EditorComponents>().types.clone();

    roots.dedup();
    for root_id in roots {
        let Some(root_entity) = world.resource::<SceneIndex>().get(&root_id) else {
            continue;
        };
        let Some(instance) = world.get::<PrefabInstance>(root_entity).copied() else {
            continue;
        };

        // Gather stamped children of this root with their template ids.
        let children: Vec<(Entity, SceneId)> = {
            let mut query = world.query::<(Entity, &StampedFrom)>();
            query
                .iter(world)
                .filter(|(_, s)| s.instance_root == root_id)
                .map(|(e, s)| (e, s.template_id))
                .collect()
        };
        let mut patches: Vec<OverridePatch> = Vec::new();
        {
            let library = world.resource::<PrefabLibrary>();
            let Some(prefab) = library.prefabs.get(&instance.0) else {
                continue;
            };
            let template: std::collections::HashMap<SceneId, &[Box<dyn PartialReflect>]> = prefab
                .template
                .records()
                .map(|(id, _, c)| (id, c))
                .collect();
            for (entity, template_id) in &children {
                let Some(template_components) = template.get(template_id) else {
                    continue;
                };
                for reg in &components {
                    let Some(registration) = registry.get(reg.type_id) else {
                        continue;
                    };
                    let Some(reflect_component) =
                        registration.data::<bevy::ecs::reflect::ReflectComponent>()
                    else {
                        continue;
                    };
                    let Ok(entity_ref) = world.get_entity(*entity) else {
                        continue;
                    };
                    let Some(current) = reflect_component.reflect(entity_ref) else {
                        continue;
                    };
                    let Some(template_value) = template_components.iter().find(|v| {
                        v.get_represented_type_info()
                            .is_some_and(|i| i.type_id() == reg.type_id)
                    }) else {
                        continue;
                    };
                    let mut diffs = Vec::new();
                    diff_fields(
                        &registry,
                        "",
                        template_value.as_ref(),
                        current.as_partial_reflect(),
                        &mut diffs,
                    );
                    for (path, value) in diffs {
                        patches.push(OverridePatch {
                            entity: template_id.0.to_string(),
                            type_path: reg.type_path.to_string(),
                            path,
                            value,
                        });
                    }
                }
            }
        }
        patches.sort_by(|a, b| {
            (&a.entity, &a.type_path, &a.path).cmp(&(&b.entity, &b.type_path, &b.path))
        });
        if let Some(mut overrides) = world.get_mut::<PrefabOverrides>(root_entity)
            && overrides.0 != patches
        {
            overrides.0 = patches;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{EditorPrefabsPlugin, PrefabDef, stamp_prefab};
    use editor_scene::snapshot_from_parts;
    use uuid::Uuid;

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component)]
    struct Payload {
        power: f32,
        speed: f32,
    }

    struct TestFeature;
    impl EditorFeature for TestFeature {
        fn manifest(&self) -> FeatureManifest {
            FeatureManifest::new("ov-test", "Override Test")
        }
        fn register(&self, reg: &mut FeatureRegistry) {
            reg.component::<Payload>().component::<Transform>();
        }
    }

    // D5 core loop: edit a stamped child -> per-field patch derived; template
    // update propagates while the overridden field stays; revert restores.
    #[test]
    fn override_loop() {
        let mut app = App::new();
        app.add_plugins(editor_core::EditorCorePlugin);
        app.add_plugins(EditorPrefabsPlugin);
        app.add_editor_feature(TestFeature);
        app.init_resource::<bevy::input::ButtonInput<bevy::input::keyboard::KeyCode>>();
        app.finish();
        app.update();

        let template_child = SceneId::random();
        let prefab_id = Uuid::new_v4();
        let prefab = PrefabDef {
            kit: None,
            id: prefab_id,
            name: "Thing".into(),
            template: snapshot_from_parts(vec![(
                template_child,
                None,
                vec![
                    Box::new(Payload {
                        power: 1.0,
                        speed: 10.0,
                    })
                    .into_partial_reflect(),
                ],
            )]),
        };
        app.world_mut()
            .resource_mut::<PrefabLibrary>()
            .prefabs
            .insert(prefab_id, prefab);

        // Place an instance.
        let root_id = SceneId::random();
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Place".into(),
                gesture: None,
                ops: vec![Op::Spawn {
                    id: root_id,
                    components: vec![
                        Box::new(PrefabInstance(prefab_id)).into_partial_reflect(),
                        Box::new(PrefabOverrides::default()).into_partial_reflect(),
                    ],
                }],
            });
        app.update();
        app.update();

        // Edit the stamped child's power through the queue (any edit path works).
        let stamped_id = {
            let world = app.world_mut();
            let mut query = world.query_filtered::<&SceneId, With<StampedFrom>>();
            *query.iter(world).next().unwrap()
        };
        app.world_mut()
            .resource_mut::<EditQueue>()
            .0
            .push(Transaction {
                label: "Edit".into(),
                gesture: None,
                ops: vec![Op::Set {
                    target: stamped_id,
                    value: Box::new(Payload {
                        power: 5.0,
                        speed: 10.0,
                    })
                    .into_partial_reflect(),
                }],
            });
        app.update();

        // Per-field patch derived onto the root.
        let world = app.world_mut();
        let root_entity = world.resource::<SceneIndex>().get(&root_id).unwrap();
        let overrides = world.get::<PrefabOverrides>(root_entity).unwrap().clone();
        assert_eq!(overrides.0.len(), 1, "one leaf diff: {:?}", overrides.0);
        assert_eq!(overrides.0[0].path, "power");
        assert_eq!(overrides.0[0].entity, template_child.0.to_string());

        // Template update (speed 10 -> 99) + restamp: override survives, the
        // non-overridden field follows the source.
        {
            let mut library = app.world_mut().resource_mut::<PrefabLibrary>();
            let prefab = library.prefabs.get_mut(&prefab_id).unwrap();
            prefab.template = snapshot_from_parts(vec![(
                template_child,
                None,
                vec![
                    Box::new(Payload {
                        power: 1.0,
                        speed: 99.0,
                    })
                    .into_partial_reflect(),
                ],
            )]);
        }
        // Re-stamp this instance (the propagation path).
        let world = app.world_mut();
        let stamped: Vec<Entity> = {
            let mut q = world.query_filtered::<Entity, With<StampedFrom>>();
            q.iter(world).collect()
        };
        for e in stamped {
            world.entity_mut(e).despawn();
        }
        let root_entity = world.resource::<SceneIndex>().get(&root_id).unwrap();
        stamp_prefab(world, prefab_id, root_entity);
        app.update();

        let world = app.world_mut();
        let payload = {
            let mut q = world.query_filtered::<&Payload, With<StampedFrom>>();
            q.iter(world).next().unwrap().clone()
        };
        assert_eq!(payload.speed, 99.0, "source edit propagated");
        assert_eq!(payload.power, 5.0, "override preserved over new template");
    }
}
