//! M0 Spike 1 — EditQueue at scale (spikes/README.md).
//!
//! Proves (or refutes) three claims from spec §5 / RFC §5:
//!   1. Reflection-based inverse capture (clone old component → apply new) is fast
//!      enough for keystroke-granularity edits on 1000-entity scenes.
//!   2. A whole-scene transaction (select-all edit) fits the 120fps frame budget.
//!   3. Gesture coalescing collapses a 60-frame drag into one undo entry cheaply.
//!
//! Throwaway code: the point is numbers + FINDINGS.md, not reuse.

use std::any::TypeId;
use std::collections::HashMap;
use std::time::Instant;

use bevy::prelude::*;
use bevy::reflect::TypeRegistry;
use uuid::Uuid;

const ENTITY_COUNT: usize = 1000;
const DRAG_FRAMES: usize = 60;
const DRAG_SELECTION: usize = 250;
const ITERATIONS: usize = 100;

// Budgets (µs): spec §8 — 1000-entity scene edits at 120fps (8.3ms frame).
const BUDGET_SINGLE_EDIT_US: u128 = 500; // one keystroke edit, well under a frame
const BUDGET_FULL_SCENE_US: u128 = 8300; // select-all edit, one frame
const BUDGET_DRAG_FRAME_US: u128 = 4000; // per-frame drag cost, half a frame

#[derive(Component, Reflect, Clone, PartialEq, Debug)]
#[reflect(Component)]
struct Health {
    current: f32,
    max: f32,
}

/// Stable identity (spec §5): edits target UUIDs, never Entity.
#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct SceneId(Uuid);

#[derive(Default)]
struct SceneIndex(HashMap<Uuid, Entity>);

/// Spike-sized EditOp: one reflection-generic op (the risky claim under test) and one
/// typed coalescable op (the drag-gesture representative).
enum Op {
    /// Set a whole component via reflection; inverse = Set of the captured old value.
    Set {
        target: Uuid,
        type_id: TypeId,
        value: Box<dyn PartialReflect>,
    },
    /// Translate a selection; inverse = negated delta. Coalesces by accumulation.
    Translate { targets: Vec<Uuid>, delta: Vec3 },
}

#[allow(dead_code, reason = "spike: label kept for debug printing")]
struct Applied {
    label: String,
    gesture: Option<u64>,
    inverse: Vec<Op>,
}

#[derive(Default)]
struct History {
    undo: Vec<Applied>,
}

fn clone_component(
    world: &World,
    registry: &TypeRegistry,
    entity: Entity,
    type_id: TypeId,
) -> Option<Box<dyn PartialReflect>> {
    let reflect_component = registry.get(type_id)?.data::<ReflectComponent>()?;
    let entity_ref = world.get_entity(entity).ok()?;
    Some(reflect_component.reflect(entity_ref)?.to_dynamic())
}

fn apply_set(
    world: &mut World,
    registry: &TypeRegistry,
    entity: Entity,
    type_id: TypeId,
    value: &dyn PartialReflect,
) {
    let reflect_component = registry
        .get(type_id)
        .and_then(|r| r.data::<ReflectComponent>())
        .expect("type registered");
    let mut entity_mut = world.get_entity_mut(entity).expect("entity exists");
    reflect_component.apply_or_insert_mapped(
        &mut entity_mut,
        value,
        registry,
        &mut (),
        bevy::ecs::relationship::RelationshipHookMode::Run,
    );
}

/// Apply a transaction: capture inverses, apply ops, push history.
/// Coalescing: a Translate-only transaction with the same gesture id as the previous
/// entry folds into it (accumulate inverse delta) instead of pushing a new entry.
fn commit(
    world: &mut World,
    index: &SceneIndex,
    history: &mut History,
    label: &str,
    gesture: Option<u64>,
    ops: Vec<Op>,
) {
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();

    // Gesture coalescing path.
    if let (Some(g), [Op::Translate { targets, delta }]) = (gesture, ops.as_slice())
        && let Some(prev) = history.undo.last_mut()
        && prev.gesture == Some(g)
        && let Some(Op::Translate { delta: inv, .. }) = prev.inverse.first_mut()
    {
        *inv -= *delta;
        apply_translate(world, index, targets, *delta);
        return;
    }

    let mut inverse = Vec::with_capacity(ops.len());
    for op in &ops {
        match op {
            Op::Set {
                target, type_id, ..
            } => {
                let entity = index.0[target];
                let old =
                    clone_component(world, &registry, entity, *type_id).expect("component present");
                inverse.push(Op::Set {
                    target: *target,
                    type_id: *type_id,
                    value: old,
                });
            }
            Op::Translate { targets, delta } => inverse.push(Op::Translate {
                targets: targets.clone(),
                delta: -*delta,
            }),
        }
    }
    for op in &ops {
        match op {
            Op::Set {
                target,
                type_id,
                value,
            } => apply_set(world, &registry, index.0[target], *type_id, value.as_ref()),
            Op::Translate { targets, delta } => apply_translate(world, index, targets, *delta),
        }
    }
    inverse.reverse();
    history.undo.push(Applied {
        label: label.to_string(),
        gesture,
        inverse,
    });
}

fn apply_translate(world: &mut World, index: &SceneIndex, targets: &[Uuid], delta: Vec3) {
    for id in targets {
        if let Some(mut t) = world.get_mut::<Transform>(index.0[id]) {
            t.translation += delta;
        }
    }
}

fn undo(world: &mut World, index: &SceneIndex, history: &mut History) {
    let Some(applied) = history.undo.pop() else {
        return;
    };
    let registry_arc = world.resource::<AppTypeRegistry>().clone();
    let registry = registry_arc.read();
    for op in &applied.inverse {
        match op {
            Op::Set {
                target,
                type_id,
                value,
            } => apply_set(world, &registry, index.0[target], *type_id, value.as_ref()),
            Op::Translate { targets, delta } => apply_translate(world, index, targets, *delta),
        }
    }
}

fn setup(world: &mut World) -> (SceneIndex, Vec<Uuid>) {
    let registry = AppTypeRegistry::default();
    {
        let mut w = registry.write();
        w.register::<Transform>();
        w.register::<Health>();
    }
    world.insert_resource(registry);

    let mut index = SceneIndex::default();
    let mut ids = Vec::with_capacity(ENTITY_COUNT);
    for i in 0..ENTITY_COUNT {
        let id = Uuid::new_v4();
        let entity = world
            .spawn((
                SceneId(id),
                Transform::from_xyz(i as f32, 0.0, 0.0),
                Health {
                    current: 100.0,
                    max: 100.0,
                },
            ))
            .id();
        index.0.insert(id, entity);
        ids.push(id);
    }
    (index, ids)
}

struct Stats {
    mean_us: u128,
    p99_us: u128,
}

fn measure(mut f: impl FnMut()) -> Stats {
    let mut samples = Vec::with_capacity(ITERATIONS);
    for _ in 0..ITERATIONS {
        let t = Instant::now();
        f();
        samples.push(t.elapsed().as_micros());
    }
    samples.sort_unstable();
    Stats {
        mean_us: samples.iter().sum::<u128>() / samples.len() as u128,
        p99_us: samples[samples.len() * 99 / 100],
    }
}

fn verdict(name: &str, stats: &Stats, budget_us: u128) {
    let ok = stats.p99_us <= budget_us;
    println!(
        "{:<38} mean {:>6}µs  p99 {:>6}µs  budget {:>6}µs  {}",
        name,
        stats.mean_us,
        stats.p99_us,
        budget_us,
        if ok { "PASS" } else { "FAIL" }
    );
}

fn main() {
    let mut world = World::new();
    let (index, ids) = setup(&mut world);
    let mut history = History::default();

    // 1. Single-entity reflection Set (keystroke-granularity inspector edit).
    let health_ty = TypeId::of::<Health>();
    let single = measure(|| {
        commit(
            &mut world,
            &index,
            &mut history,
            "Edit health",
            None,
            vec![Op::Set {
                target: ids[0],
                type_id: health_ty,
                value: Box::new(Health {
                    current: 50.0,
                    max: 120.0,
                })
                .into_partial_reflect(),
            }],
        );
    });
    verdict("single reflection Set", &single, BUDGET_SINGLE_EDIT_US);

    // 2. Full-scene reflection transaction (select-all component edit): capture 1000
    //    inverses via reflection + apply 1000 sets, one transaction.
    let full = measure(|| {
        let ops = ids
            .iter()
            .map(|id| Op::Set {
                target: *id,
                type_id: health_ty,
                value: Box::new(Health {
                    current: 75.0,
                    max: 150.0,
                })
                .into_partial_reflect(),
            })
            .collect();
        commit(
            &mut world,
            &index,
            &mut history,
            "Select-all edit",
            None,
            ops,
        );
    });
    verdict(
        "1000-entity reflection transaction",
        &full,
        BUDGET_FULL_SCENE_US,
    );

    // 3. Simulated drag: 60 frames of Translate on 250 entities, one gesture id.
    //    Measures per-frame cost; asserts coalescing → exactly one history entry.
    let selection: Vec<Uuid> = ids.iter().take(DRAG_SELECTION).copied().collect();
    let pre_drag: Vec<Vec3> = selection
        .iter()
        .map(|id| world.get::<Transform>(index.0[id]).unwrap().translation)
        .collect();
    let history_before = history.undo.len();
    let mut frame_costs = Vec::with_capacity(DRAG_FRAMES);
    for frame in 0..DRAG_FRAMES {
        let t = Instant::now();
        commit(
            &mut world,
            &index,
            &mut history,
            "Move selection",
            Some(42),
            vec![Op::Translate {
                targets: selection.clone(),
                delta: Vec3::new(0.01, 0.0, if frame % 2 == 0 { 0.01 } else { -0.01 }),
            }],
        );
        frame_costs.push(t.elapsed().as_micros());
    }
    frame_costs.sort_unstable();
    let drag = Stats {
        mean_us: frame_costs.iter().sum::<u128>() / frame_costs.len() as u128,
        p99_us: frame_costs[frame_costs.len() * 99 / 100],
    };
    verdict(
        "drag frame (250 entities, coalesced)",
        &drag,
        BUDGET_DRAG_FRAME_US,
    );
    let entries = history.undo.len() - history_before;
    println!(
        "{:<38} {} entr{} for {} frames              {}",
        "gesture coalescing",
        entries,
        if entries == 1 { "y" } else { "ies" },
        DRAG_FRAMES,
        if entries == 1 { "PASS" } else { "FAIL" }
    );

    // 4. Undo correctness + latency: undo must restore the exact pre-drag positions
    //    (coalesced inverse = accumulated negated deltas; float error must stay tiny).
    let t = Instant::now();
    undo(&mut world, &index, &mut history);
    let undo_us = t.elapsed().as_micros();
    let max_err = selection
        .iter()
        .zip(&pre_drag)
        .map(|(id, orig)| {
            (world.get::<Transform>(index.0[id]).unwrap().translation - *orig).length()
        })
        .fold(0.0f32, f32::max);
    println!(
        "{:<38} {}µs, max restore error {:.6}      {}",
        "undo coalesced drag",
        undo_us,
        max_err,
        if max_err < 1e-3 { "PASS" } else { "FAIL" }
    );

    // 5. Undo of the full-scene transaction.
    let t = Instant::now();
    undo(&mut world, &index, &mut history);
    println!(
        "{:<38} {}µs",
        "undo 1000-entity transaction",
        t.elapsed().as_micros()
    );

    println!("\nhistory depth after run: {}", history.undo.len());
}
