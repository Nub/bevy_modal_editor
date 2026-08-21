//! `*` — select every object like this one (spec §9, keymap doc §Normal).
//!
//! The kernel cannot name a prefab, a model or a game type, so "like this one"
//! is a ladder of registered reflected comparisons — see
//! `editor_api::identity`. Features declare their rungs; this compares them.
//!
//! It is not an edit. Nothing here queues a `Transaction`, so `*` never lands
//! in undo and never dirties the scene.

use crate::hide::{Hidden, HideRequests};
use crate::selection::{Selected, SelectionHandle, SelectionScope};
use bevy::prelude::*;
use bevy::reflect::{PartialReflect, TypeRegistry};
use editor_api::identity::IdentityDef;
use editor_api::prelude::*;

/// The identity ladder, in priority order, built once at startup.
#[derive(Resource, Default)]
pub struct IdentityCatalog {
    pub rungs: Vec<IdentityDef>,
}

/// What one object IS, for comparison purposes.
struct Identity {
    type_path: &'static str,
    noun: &'static str,
    key: &'static str,
    /// `None` for a presence rung — the component's value is irrelevant.
    value: Option<Box<dyn PartialReflect>>,
}

fn reflected<'w>(
    world: &'w World,
    entity: Entity,
    registry: &TypeRegistry,
    def: &IdentityDef,
) -> Option<&'w dyn bevy::reflect::Reflect> {
    let registration = registry.get(def.component)?;
    let reflect_component = registration.data::<bevy::ecs::reflect::ReflectComponent>()?;
    reflect_component.reflect(world.get_entity(entity).ok()?)
}

fn value_of(
    component: &dyn bevy::reflect::Reflect,
    key: &str,
) -> Option<Option<Box<dyn PartialReflect>>> {
    if key == "*" {
        return Some(None);
    }
    if key.is_empty() {
        return Some(Some(component.as_partial_reflect().to_dynamic()));
    }
    let path = bevy::reflect::ParsedPath::parse(key).ok()?;
    let field = path.reflect_element(component.as_partial_reflect()).ok()?;
    Some(Some(field.to_dynamic()))
}

/// The first rung whose component is present decides. A barrel that is both a
/// prefab instance and carries a mesh is a barrel.
fn identity_of(
    world: &World,
    entity: Entity,
    catalog: &IdentityCatalog,
    registry: &TypeRegistry,
) -> Option<Identity> {
    for def in &catalog.rungs {
        let Some(component) = reflected(world, entity, registry, def) else {
            continue;
        };
        let Some(value) = value_of(component, def.key) else {
            continue;
        };
        return Some(Identity {
            type_path: def.type_path,
            noun: def.noun,
            key: def.key,
            value,
        });
    }
    None
}

fn matches(
    identity: &Identity,
    world: &World,
    entity: Entity,
    catalog: &IdentityCatalog,
    registry: &TypeRegistry,
) -> bool {
    let Some(def) = catalog
        .rungs
        .iter()
        .find(|d| d.type_path == identity.type_path && d.key == identity.key)
    else {
        return false;
    };
    let Some(component) = reflected(world, entity, registry, def) else {
        return false;
    };
    let Some(value) = value_of(component, def.key) else {
        return false;
    };
    match (&identity.value, &value) {
        // A presence rung: carrying the component IS the match.
        (None, None) => true,
        (Some(wanted), Some(found)) => {
            // reflect_partial_eq, not Hash/Eq: a game type must not have to
            // implement anything to join the ladder.
            wanted.reflect_partial_eq(found.as_ref()) == Some(true)
        }
        _ => false,
    }
}

fn say(world: &mut World, message: String, success: bool) {
    world.write_message(editor_api::feedback::SceneIoFeedback { message, success });
}

pub(crate) fn perform_select_similar(world: &mut World) {
    if !std::mem::take(&mut world.resource_mut::<HideRequests>().similar) {
        return;
    }
    if !world.resource::<crate::resolver::EditorState>().active {
        return;
    }
    let sources: Vec<Entity> = world
        .query_filtered::<Entity, (With<Selected>, With<SceneId>)>()
        .iter(world)
        .collect();
    if sources.is_empty() {
        say(
            world,
            "select something first \u{b7} * selects every one like it".into(),
            false,
        );
        return;
    }
    // A socket is a HANDLE: it clicks as itself, so `outermost_seal` returns it
    // unchanged and every socket in the file would come back as one family.
    if sources
        .iter()
        .any(|e| world.get::<SelectionHandle>(*e).is_some())
    {
        say(world, "* works on objects, not sockets".into(), false);
        return;
    }

    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    let catalog = std::mem::take(&mut *world.resource_mut::<IdentityCatalog>());

    let mut identities: Vec<Identity> = Vec::new();
    for entity in &sources {
        let Some(identity) = identity_of(world, *entity, &catalog, &registry) else {
            continue;
        };
        let duplicate = identities.iter().any(|seen| {
            seen.type_path == identity.type_path
                && seen.key == identity.key
                && match (&seen.value, &identity.value) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.reflect_partial_eq(b.as_ref()) == Some(true),
                    _ => false,
                }
        });
        if !duplicate {
            identities.push(identity);
        }
    }
    if identities.is_empty() {
        drop(registry);
        world.insert_resource(catalog);
        say(
            world,
            "nothing identifies this object \u{b7} * matches prefabs, models and kinds".into(),
            false,
        );
        return;
    }

    let scoped = world.resource::<SelectionScope>().0.is_some();
    let mut skipped_hidden = 0usize;
    let mut result: Vec<Entity> = sources.clone();
    {
        let hidden = world.resource::<Hidden>().clone();
        for (_, entity) in crate::hide::candidates(world) {
            if result.contains(&entity) {
                continue;
            }
            if !identities
                .iter()
                .any(|identity| matches(identity, world, entity, &catalog, &registry))
            {
                continue;
            }
            // A hidden object is out of the conversation: selecting what you
            // cannot see is how a batch edit reaches something by accident.
            if crate::hide::is_hidden_world(world, entity, &hidden) {
                skipped_hidden += 1;
                continue;
            }
            result.push(entity);
        }
    }
    drop(registry);

    let noun = identities[0].noun;
    let families = identities.len();
    world.insert_resource(catalog);

    if result.len() == sources.len() {
        let where_ = if scoped {
            "in the open prefab"
        } else {
            "in the level"
        };
        say(world, format!("no other {noun} {where_}"), false);
        return;
    }

    for entity in world
        .query_filtered::<Entity, With<Selected>>()
        .iter(world)
        .collect::<Vec<_>>()
    {
        world.entity_mut(entity).remove::<Selected>();
    }
    let locked = result
        .iter()
        .filter(|e| world.get::<crate::lock::Locked>(**e).is_some())
        .count();
    for entity in &result {
        world.entity_mut(*entity).insert(Selected);
    }
    world.write_message(crate::selection::SelectionChanged);

    let count = result.len();
    let mut message = if families == 1 {
        format!("selected {count} \u{b7} {noun}")
    } else {
        format!("selected {count} \u{b7} {families} families")
    };
    if skipped_hidden > 0 {
        message.push_str(&format!(" \u{b7} {skipped_hidden} hidden skipped"));
    }
    if locked > 0 {
        message.push_str(&format!(" \u{b7} {locked} locked"));
    }
    if scoped {
        message.push_str(" \u{b7} in the open prefab");
    }
    say(world, message, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component, Default)]
    struct Shape {
        kind: u32,
        size: f32,
    }

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component, Default)]
    struct Volume {
        name: String,
    }

    #[derive(Component, Reflect, Default, Clone, PartialEq, Debug)]
    #[reflect(Component, Default)]
    struct Badge(u32);

    fn world_with_rungs(rungs: Vec<IdentityDef>) -> (World, IdentityCatalog) {
        let mut world = World::new();
        let registry = AppTypeRegistry::default();
        {
            let mut write = registry.write();
            write.register::<Shape>();
            write.register::<Volume>();
            write.register::<Badge>();
        }
        world.insert_resource(registry);
        (world, IdentityCatalog { rungs })
    }

    fn rung<T: bevy::reflect::TypePath>(priority: u32, key: &'static str) -> IdentityDef {
        IdentityDef {
            priority,
            component: std::any::TypeId::of::<T>(),
            type_path: T::type_path(),
            key,
            noun: "thing",
        }
    }

    /// A key-path rung compares ONE field. Two cubes of different sizes are
    /// both cubes — the whole point of naming a key rather than the component.
    #[test]
    fn a_key_path_rung_ignores_the_other_fields() {
        let (mut world, catalog) = world_with_rungs(vec![rung::<Shape>(1, "kind")]);
        let a = world.spawn(Shape { kind: 1, size: 1.0 }).id();
        let b = world.spawn(Shape { kind: 1, size: 9.0 }).id();
        let c = world.spawn(Shape { kind: 2, size: 1.0 }).id();
        let arc = world.resource::<AppTypeRegistry>().clone();
        let registry = arc.read();
        let identity = identity_of(&world, a, &catalog, &registry).expect("identity");
        assert!(matches(&identity, &world, b, &catalog, &registry));
        assert!(
            !matches(&identity, &world, c, &catalog, &registry),
            "a different kind matched"
        );
    }

    /// A presence rung matches on the component alone: two trigger volumes are
    /// the same kind of thing even though one is "lift" and one is "pit".
    #[test]
    fn a_presence_rung_matches_on_presence_only() {
        let (mut world, catalog) = world_with_rungs(vec![rung::<Volume>(1, "*")]);
        let a = world
            .spawn(Volume {
                name: "lift".into(),
            })
            .id();
        let b = world.spawn(Volume { name: "pit".into() }).id();
        let c = world.spawn(Shape::default()).id();
        let arc = world.resource::<AppTypeRegistry>().clone();
        let registry = arc.read();
        let identity = identity_of(&world, a, &catalog, &registry).expect("identity");
        assert!(matches(&identity, &world, b, &catalog, &registry));
        assert!(!matches(&identity, &world, c, &catalog, &registry));
    }

    /// A whole-value rung is exact.
    #[test]
    fn a_whole_value_rung_needs_the_whole_value() {
        let (mut world, catalog) = world_with_rungs(vec![rung::<Shape>(1, "")]);
        let a = world.spawn(Shape { kind: 1, size: 1.0 }).id();
        let same = world.spawn(Shape { kind: 1, size: 1.0 }).id();
        let bigger = world.spawn(Shape { kind: 1, size: 2.0 }).id();
        let arc = world.resource::<AppTypeRegistry>().clone();
        let registry = arc.read();
        let identity = identity_of(&world, a, &catalog, &registry).expect("identity");
        assert!(matches(&identity, &world, same, &catalog, &registry));
        assert!(!matches(&identity, &world, bigger, &catalog, &registry));
    }

    /// An object carrying two rungs is the HIGHER-priority one. A barrel that
    /// is a prefab instance and also carries a mesh is a barrel, not a mesh.
    #[test]
    fn rungs_are_consulted_in_priority_order() {
        let (mut world, catalog) =
            world_with_rungs(vec![rung::<Badge>(1, "*"), rung::<Shape>(2, "kind")]);
        let both = world.spawn((Badge(7), Shape { kind: 3, size: 1.0 })).id();
        let arc = world.resource::<AppTypeRegistry>().clone();
        let registry = arc.read();
        let identity = identity_of(&world, both, &catalog, &registry).expect("identity");
        assert_eq!(identity.type_path, Badge::type_path());
    }
}
