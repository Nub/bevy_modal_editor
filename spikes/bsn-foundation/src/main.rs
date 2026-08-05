//! M0 Spike 2 — bsn-foundation (spikes/README.md).
//!
//! Proves (or refutes) the BSN-first policy's load-bearing claims (spec §5):
//!   A. BSN per-field patches express prefab instance overrides.
//!   B. Inheritance-by-inclusion expresses prefab variants (last-write-wins per field).
//!   C. `SceneId` (UUID) components round-trip through BSN spawning.
//!   D. A versioned envelope (our serde format) can drive BSN at runtime: deserialize
//!      → look up prefab + patch appliers in registries → spawn — the community
//!      ".bsn loader" pattern, which is exactly what the editor needs.
//!
//! Throwaway code; conclusions go to FINDINGS.md and docs/bsn-ledger.md.

use bevy::asset::AssetPlugin;
use bevy::prelude::*;
use bevy::scene::{PatchFromTemplate, Scene, WorldSceneExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// Blanket FromTemplate via Default + Clone (report §4).
#[derive(Component, Default, Clone, PartialEq, Debug)]
struct Health {
    current: f32,
    max: f32,
}

#[derive(Component, Default, Clone, PartialEq, Debug)]
struct SceneId(Uuid);

fn check(name: &str, ok: bool) {
    println!("{:<58} {}", name, if ok { "PASS" } else { "FAIL" });
    if !ok {
        std::process::exit(1);
    }
}

// "Prefab" = a function returning a Scene (report §5: inclusion is inheritance).
fn barrel() -> impl Scene {
    (
        bevy::scene::template_value(Health {
            current: 100.0,
            max: 100.0,
        }),
        bevy::scene::template_value(Transform::from_xyz(0.0, 0.0, 0.0)),
    )
}

// Variant = include base, patch fields on top.
fn exploding_barrel() -> impl Scene {
    (
        barrel(),
        Health::patch(|t, _| t.max = 50.0),
        bevy::scene::template_value(Name::new("ExplodingBarrel")),
    )
}

// ---------- D: the envelope + registry (the editor/.bsn-loader pattern) ----------

#[derive(Serialize, Deserialize)]
struct Envelope {
    format_version: u32,
    entities: Vec<EntityRecord>,
}

/// BSN-semantic instance record: prefab reference + per-field override deltas —
/// never an expanded tree (spec §6).
#[derive(Serialize, Deserialize)]
struct EntityRecord {
    id: Uuid,
    prefab: String,
    overrides: Vec<FieldPatch>,
}

#[derive(Serialize, Deserialize)]
struct FieldPatch {
    component: String,
    field: String,
    value: f32, // spike-simple; real impl uses reflected values
}

/// The two registries editor_api component/prefab registration would populate.
/// Values are plain `fn` pointers → this is buildable as a static table, matching the
/// `fn() -> Box<dyn ErasedComponentTemplate>` shape BSN's own erased API wants.
type PrefabFn = fn() -> Box<dyn Scene>;
type PatchFn = fn(f32) -> Box<dyn Scene>;

fn registries() -> (
    HashMap<&'static str, PrefabFn>,
    HashMap<(&'static str, &'static str), PatchFn>,
) {
    let mut prefabs: HashMap<&'static str, PrefabFn> = HashMap::new();
    prefabs.insert("barrel", || Box::new(barrel()));
    prefabs.insert("exploding_barrel", || Box::new(exploding_barrel()));

    let mut patches: HashMap<(&'static str, &'static str), PatchFn> = HashMap::new();
    patches.insert(("Health", "max"), |v| {
        Box::new(Health::patch(move |t, _| t.max = v))
    });
    patches.insert(("Health", "current"), |v| {
        Box::new(Health::patch(move |t, _| t.current = v))
    });
    patches.insert(("Transform", "x"), |v| {
        Box::new(Transform::patch(move |t, _| t.translation.x = v))
    });
    (prefabs, patches)
}

fn main() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        AssetPlugin::default(),
        bevy::scene::ScenePlugin,
    ));
    let world = app.world_mut();

    // A. Instance override: spawn prefab + per-field patch; untouched fields survive.
    let e = world
        .spawn_scene((barrel(), Health::patch(|t, _| t.max = 200.0)))
        .expect("spawn")
        .id();
    let h = world.get::<Health>(e).unwrap();
    check(
        "A  per-field override merges (current kept, max set)",
        *h == Health {
            current: 100.0,
            max: 200.0,
        },
    );

    // B. Variant chain: base -> variant -> instance override; document order wins.
    let e = world
        .spawn_scene((exploding_barrel(), Health::patch(|t, _| t.current = 25.0)))
        .expect("spawn")
        .id();
    let h = world.get::<Health>(e).unwrap();
    let name_ok = world
        .get::<Name>(e)
        .is_some_and(|n| n.as_str() == "ExplodingBarrel");
    check(
        "B  variant inheritance (base->variant->instance)",
        *h == Health {
            current: 25.0,
            max: 50.0,
        } && name_ok,
    );

    // C. SceneId UUID round-trip through BSN spawn.
    let id = Uuid::new_v4();
    let e = world
        .spawn_scene((barrel(), bevy::scene::template_value(SceneId(id))))
        .expect("spawn")
        .id();
    check(
        "C  SceneId(Uuid) rides BSN spawning",
        world.get::<SceneId>(e).is_some_and(|s| s.0 == id),
    );

    // D. Envelope round-trip: RON -> registries -> BSN spawn.
    let envelope = Envelope {
        format_version: 1,
        entities: vec![
            EntityRecord {
                id: Uuid::new_v4(),
                prefab: "barrel".into(),
                overrides: vec![
                    FieldPatch {
                        component: "Health".into(),
                        field: "max".into(),
                        value: 300.0,
                    },
                    FieldPatch {
                        component: "Transform".into(),
                        field: "x".into(),
                        value: 7.5,
                    },
                ],
            },
            EntityRecord {
                id: Uuid::new_v4(),
                prefab: "exploding_barrel".into(),
                overrides: vec![],
            },
        ],
    };
    let text = ron::ser::to_string_pretty(&envelope, Default::default()).unwrap();
    let parsed: Envelope = ron::from_str(&text).unwrap();
    check(
        "D1 envelope serializes/deserializes (RON)",
        parsed.format_version == 1,
    );

    let (prefabs, patchers) = registries();
    let mut spawned = Vec::new();
    for record in &parsed.entities {
        let base: Box<dyn Scene> = prefabs[record.prefab.as_str()]();
        // Runtime-count patches: fold into nested boxed tuples (no Vec<Scene> impl).
        let scene = record.overrides.iter().fold(base, |acc, p| {
            let patch = patchers[&(p.component.as_str(), p.field.as_str())](p.value);
            Box::new((acc, patch)) as Box<dyn Scene>
        });
        let uuid_patch = bevy::scene::template_value(SceneId(record.id));
        let e = world.spawn_scene((scene, uuid_patch)).expect("spawn").id();
        spawned.push(e);
    }
    let h0 = world.get::<Health>(spawned[0]).unwrap();
    let t0 = world.get::<Transform>(spawned[0]).unwrap();
    check(
        "D2 deserialized overrides drive BSN patches",
        *h0 == Health {
            current: 100.0,
            max: 300.0,
        } && t0.translation.x == 7.5,
    );
    let h1 = world.get::<Health>(spawned[1]).unwrap();
    let id_ok = world
        .get::<SceneId>(spawned[1])
        .is_some_and(|s| s.0 == parsed.entities[1].id);
    check(
        "D3 variant prefab from envelope + SceneId assigned",
        *h1 == Health {
            current: 100.0,
            max: 50.0,
        } && id_ok,
    );

    println!("\nall claims hold — see FINDINGS.md");
}
