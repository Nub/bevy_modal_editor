//! Trigger volumes (spec §9 — data-driven gameplay authoring).
//!
//! The editor can author how a level LOOKS and how it MOVES. Nothing a designer
//! placed could make anything HAPPEN. A named box that notices when something
//! enters it is the smallest primitive that closes that gap, and it is the one
//! every prototype reaches for first: doors, checkpoints, pickups, kill planes,
//! the moment a cutscene starts.
//!
//! It lives in `game_framework` rather than the editor deliberately, and that is
//! the interesting part. The timeline ships with the editor and therefore cannot
//! fire in a release build (§9's OWED note); a trigger is a plain component and
//! two messages, so it runs in the shipped artifact from its first commit. This
//! is the shape the rest of the runtime split has to take.
//!
//! The volume says WHEN and WHAT — a name — and never what it means. Matching on
//! the name is the whole interface, exactly as it is for `FireEffect`.
//!
//! **Deliberate limits, so they are choices rather than surprises:**
//! - An actor is a POINT at its entity origin. Bounds would mean a collider
//!   dependency or a second size field; an actor that needs coverage carries
//!   several child markers instead.
//! - Occupancy is sampled once per frame, so a volume thinner than one frame of
//!   motion can be missed. The level validator warns about volumes that thin.
//! - A name is a cue, not an identity: two volumes sharing a name both fire it,
//!   and `once` is per volume rather than per name.
//! - Ceasing to exist is leaving. An actor despawned inside a volume produces an
//!   exit carrying an `Entity` that is already dead, so no reader may assume the
//!   entity is alive — only that it is no longer inside.

use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::AppState;

/// Whether the GAME, rather than the editor, owns the world this frame.
///
/// A release build has no editor, so this is true and stays true. The editor
/// overlay (when compiled in and active) sets it false, which is what keeps a
/// designer dragging the player through a checkpoint from tripping it.
///
/// It is deliberately narrow: it says who owns the world, NOT that gameplay is
/// running. Pause is expressed in `Time<Virtual>` instead, so a paused play
/// session leaves this true while nothing steps — [`gameplay_is_live`] is the
/// condition that means what it says. When §3's `Session` sub-states arrive they
/// replace this flag; it is the interim stand-in, not a parallel fiction.
#[derive(Resource, Debug)]
pub struct GameplayActive(pub bool);

impl Default for GameplayActive {
    fn default() -> Self {
        Self(true)
    }
}

/// Gameplay is stepping: the game owns the world, time is not paused, and we are
/// in a level. A run condition rather than an early return, per spec §8.
pub fn gameplay_is_live(
    active: Res<GameplayActive>,
    time: Res<Time<Virtual>>,
    state: Res<State<AppState>>,
) -> bool {
    active.0 && !time.is_paused() && *state.get() == AppState::InGame
}

/// A named region that notices what is inside it.
///
/// **Its size is its `Transform.scale`** — the volume is a unit cube, so `s`
/// resizes it, `g` moves it and `r` turns it, and the inspector shows one
/// number that means what the viewport shows. A separate `half_extents` field
/// would be a second dial for the same quantity, and the two would disagree the
/// first time anyone used the scale gesture.
#[derive(Component, Reflect, Clone, PartialEq, Debug)]
#[reflect(Component, Default)]
#[require(TriggerState)]
pub struct TriggerVolume {
    /// The cue. This is what a game matches on, and the only thing it learns.
    pub name: String,
    /// Fire once and then be spent — a checkpoint, not a doorway.
    ///
    /// The volume still reports the exit that closes its one entry, so the
    /// invariant every reader depends on holds: **each `TriggerEntered` is
    /// followed by exactly one `TriggerExited`.** A door that opens while
    /// occupied would otherwise leak +1 forever the first time it met a
    /// one-shot volume.
    pub once: bool,
}

impl Default for TriggerVolume {
    fn default() -> Self {
        Self {
            name: "trigger".into(),
            once: false,
        }
    }
}

/// What can trip a volume. Without this marker an entity passes through
/// unnoticed, which is what keeps a volume from firing on every particle,
/// every prop and every light that drifts across it.
#[derive(Component, Reflect, Clone, Copy, Default, Debug)]
#[reflect(Component, Default)]
pub struct TriggerActor;

/// Runtime occupancy. Reflected so a tool can SEE it, deliberately never
/// registered as an editor component: who is standing in a volume is a fact
/// about this instant, not about the level, and it must not serialize.
#[derive(Component, Reflect, Default, Debug)]
#[reflect(Component, Default)]
pub struct TriggerState {
    pub inside: Vec<Entity>,
    /// Set once a `once` volume has fired and its occupant has left. Spent, not
    /// armed-and-quiet.
    pub spent: bool,
    /// The actor a `once` volume latched onto, until it leaves.
    pub latched: Option<Entity>,
}

/// Something crossed in. Games read this like any other message.
#[derive(Message, Clone, Debug)]
pub struct TriggerEntered {
    pub name: String,
    pub volume: Entity,
    pub actor: Entity,
}

/// Something crossed out. The `actor` may already be despawned — see the module
/// note: ceasing to exist is leaving.
#[derive(Message, Clone, Debug)]
pub struct TriggerExited {
    pub name: String,
    pub volume: Entity,
    pub actor: Entity,
}

/// Is `point` inside the unit box described by `volume`?
///
/// The point is pushed into the volume's local space rather than the box being
/// pushed into the world, which is what makes a rotated volume a rotated box
/// instead of an axis-aligned one that lies.
///
/// A volume flattened to nothing on any axis contains NOTHING, checked rather
/// than left to the NaN that a singular matrix would otherwise produce. A kill
/// plane authored by squashing a box to zero thickness is a real thing a
/// designer does, and "it silently never fires" is the worst possible answer.
pub fn contains(volume: &GlobalTransform, point: Vec3) -> bool {
    let scale = volume.scale();
    if scale.x.abs() < f32::EPSILON || scale.y.abs() < f32::EPSILON || scale.z.abs() < f32::EPSILON
    {
        return false;
    }
    let local = volume.affine().inverse().transform_point3(point);
    local.x.abs() <= 0.5 && local.y.abs() <= 0.5 && local.z.abs() <= 0.5
}

/// The edges between last frame's occupants and this frame's, appended to
/// caller-owned buffers.
///
/// Pure so edge detection — the part of this that goes wrong — can be proven
/// right, and buffered so a level full of volumes allocates nothing per frame
/// (spec §8).
pub fn edges(
    previous: &[Entity],
    current: &[Entity],
    entered: &mut Vec<Entity>,
    exited: &mut Vec<Entity>,
) {
    entered.extend(current.iter().filter(|a| !previous.contains(a)));
    exited.extend(previous.iter().filter(|a| !current.contains(a)));
}

fn update_triggers(
    mut volumes: Query<(Entity, &TriggerVolume, &GlobalTransform, &mut TriggerState)>,
    actors: Query<(Entity, &GlobalTransform), With<TriggerActor>>,
    mut entered_out: MessageWriter<TriggerEntered>,
    mut exited_out: MessageWriter<TriggerExited>,
    mut current: Local<Vec<Entity>>,
    mut entered: Local<Vec<Entity>>,
    mut exited: Local<Vec<Entity>>,
) {
    for (volume_entity, volume, global, mut state) in &mut volumes {
        if state.spent {
            continue;
        }
        current.clear();
        entered.clear();
        exited.clear();
        current.extend(
            actors
                .iter()
                .filter(|(entity, actor)| {
                    // A latched one-shot volume is watching ONE actor: whoever
                    // else wanders through is no longer its business.
                    state.latched.is_none_or(|latched| latched == *entity)
                        && contains(global, actor.translation())
                })
                .map(|(entity, _)| entity),
        );
        edges(&state.inside, &current, &mut entered, &mut exited);
        for actor in exited.drain(..) {
            exited_out.write(TriggerExited {
                name: volume.name.clone(),
                volume: volume_entity,
                actor,
            });
            if state.latched == Some(actor) {
                // Its one entry has been closed. Now it is over.
                state.spent = true;
            }
        }
        for actor in entered.drain(..) {
            if state.spent || (volume.once && state.latched.is_some()) {
                // A one-shot volume fires for the FIRST actor across, not for
                // everyone who happened to cross on the same frame.
                continue;
            }
            entered_out.write(TriggerEntered {
                name: volume.name.clone(),
                volume: volume_entity,
                actor,
            });
            if volume.once {
                state.latched = Some(actor);
            }
        }
        state.inside.clear();
        state.inside.extend(current.iter().copied());
        if state.spent {
            state.inside.clear();
        }
    }
}

pub(crate) fn plugin(app: &mut App) {
    app.init_resource::<GameplayActive>();
    app.register_type::<TriggerVolume>();
    app.register_type::<TriggerActor>();
    app.register_type::<TriggerState>();
    app.add_message::<TriggerEntered>();
    app.add_message::<TriggerExited>();
    // PostUpdate, after propagation, for a reason that is not tidiness: a
    // freshly spawned actor carries an IDENTITY `GlobalTransform` until
    // propagation runs, so an `Update` system reads every new actor as standing
    // at the world origin — and any volume over the origin fires a phantom
    // entry on the frame the level loads, spending a one-shot before the player
    // has moved. Running after propagation also gives the system an unambiguous
    // order against the editor's play/pause handoff, which lives in `Update`.
    app.add_systems(
        PostUpdate,
        update_triggers
            .after(TransformSystems::Propagate)
            .run_if(gameplay_is_live),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A world where transforms actually propagate and states actually exist —
    /// this feature's two worst bugs both lived in the gap between "I wrote a
    /// Transform" and "the world agrees where that entity is".
    fn trigger_app() -> App {
        let mut app = App::new();
        app.add_plugins((
            bevy::time::TimePlugin,
            bevy::transform::TransformPlugin,
            bevy::state::app::StatesPlugin,
        ));
        app.init_state::<AppState>();
        plugin(&mut app);
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::InGame);
        app.update();
        app
    }

    fn volume(app: &mut App, name: &str, at: Vec3, size: f32, once: bool) -> Entity {
        app.world_mut()
            .spawn((
                TriggerVolume {
                    name: name.into(),
                    once,
                },
                Transform::from_translation(at).with_scale(Vec3::splat(size)),
            ))
            .id()
    }

    fn actor(app: &mut App, at: Vec3) -> Entity {
        app.world_mut()
            .spawn((TriggerActor, Transform::from_translation(at)))
            .id()
    }

    fn move_to(app: &mut App, entity: Entity, to: Vec3) {
        app.world_mut()
            .entity_mut(entity)
            .insert(Transform::from_translation(to));
    }

    fn entered(app: &mut App) -> Vec<String> {
        let mut messages = app.world_mut().resource_mut::<Messages<TriggerEntered>>();
        messages.drain().map(|m| m.name).collect()
    }

    fn exited(app: &mut App) -> Vec<String> {
        let mut messages = app.world_mut().resource_mut::<Messages<TriggerExited>>();
        messages.drain().map(|m| m.name).collect()
    }

    #[test]
    fn a_box_contains_what_is_inside_it() {
        let at = GlobalTransform::from(
            Transform::from_xyz(10.0, 0.0, 0.0).with_scale(Vec3::new(2.0, 4.0, 2.0)),
        );
        assert!(contains(&at, Vec3::new(10.0, 1.9, 0.0)), "inside");
        assert!(!contains(&at, Vec3::new(10.0, 2.1, 0.0)), "over the top");
        assert!(!contains(&at, Vec3::new(11.5, 0.0, 0.0)), "past the side");
        // The boundary belongs to the volume: a checkpoint you are standing
        // exactly on has been reached.
        assert!(contains(&at, Vec3::new(11.0, 2.0, 1.0)), "the corner");
    }

    #[test]
    fn rotating_the_volume_rotates_the_box() {
        // A long thin slab turned 90° about Y: what was inside along X is now
        // outside, and what was outside along Z is now inside. An axis-aligned
        // test passes both of these and is wrong twice.
        let at = GlobalTransform::from(
            Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
                .with_scale(Vec3::new(8.0, 2.0, 1.0)),
        );
        assert!(!contains(&at, Vec3::new(3.0, 0.0, 0.0)));
        assert!(contains(&at, Vec3::new(0.0, 0.0, 3.0)));
    }

    /// A kill plane is authored by squashing a box flat, and a flattened
    /// affine is singular: every comparison against NaN is false, so it would
    /// work "by accident" — until glam's debug assertions turn it into a panic.
    #[test]
    fn a_volume_flattened_to_nothing_contains_nothing() {
        let flat =
            GlobalTransform::from(Transform::default().with_scale(Vec3::new(20.0, 0.0, 20.0)));
        assert!(!contains(&flat, Vec3::ZERO), "not even its own centre");
    }

    #[test]
    fn edges_are_the_difference_between_two_frames() {
        let a = Entity::from_raw_u32(1).unwrap();
        let b = Entity::from_raw_u32(2).unwrap();
        let c = Entity::from_raw_u32(3).unwrap();
        let (mut entered, mut exited) = (Vec::new(), Vec::new());
        edges(&[a, b], &[b, c], &mut entered, &mut exited);
        assert_eq!(entered, vec![c], "c walked in");
        assert_eq!(exited, vec![a], "a walked out");
        entered.clear();
        exited.clear();
        edges(&[a], &[a], &mut entered, &mut exited);
        assert!(
            entered.is_empty() && exited.is_empty(),
            "standing still is not an event"
        );
    }

    #[test]
    fn a_volume_fires_on_the_crossing_and_not_every_frame() {
        let mut app = trigger_app();
        let volume_entity = volume(&mut app, "checkpoint", Vec3::ZERO, 2.0, false);
        let walker = actor(&mut app, Vec3::new(9.0, 0.0, 0.0));
        app.update();
        assert!(entered(&mut app).is_empty(), "far away");

        move_to(&mut app, walker, Vec3::ZERO);
        app.update();
        assert_eq!(entered(&mut app), vec!["checkpoint".to_string()]);
        assert_eq!(
            app.world()
                .get::<TriggerState>(volume_entity)
                .unwrap()
                .inside,
            vec![walker]
        );

        app.update();
        assert!(entered(&mut app).is_empty(), "standing still is quiet");

        move_to(&mut app, walker, Vec3::new(9.0, 0.0, 0.0));
        app.update();
        assert_eq!(exited(&mut app), vec!["checkpoint".to_string()]);
    }

    /// The bug this feature would have shipped with: `GlobalTransform`
    /// propagates in `PostUpdate`, so an entity spawned this frame reads as
    /// standing at the WORLD ORIGIN until it is propagated. Any volume over the
    /// origin fires a phantom entry on the frame the level loads — and spends a
    /// one-shot before the player has moved a step.
    #[test]
    fn a_freshly_spawned_actor_is_not_at_the_origin() {
        let mut app = trigger_app();
        volume(&mut app, "cutscene", Vec3::ZERO, 4.0, true);
        actor(&mut app, Vec3::new(40.0, 0.0, 40.0));
        app.update();
        assert!(
            entered(&mut app).is_empty(),
            "an actor spawned far away has never been near the origin"
        );
    }

    #[test]
    fn an_unmarked_entity_passes_through_unnoticed() {
        let mut app = trigger_app();
        volume(&mut app, "trigger", Vec3::ZERO, 2.0, false);
        // No `TriggerActor`: a prop, a particle, a light drifting across.
        app.world_mut().spawn(Transform::default());
        app.update();
        app.update();
        assert!(entered(&mut app).is_empty());
    }

    /// `once` means one entry — including when two actors arrive on the SAME
    /// frame, which is the case a `spent` flag set inside the loop gets wrong.
    #[test]
    fn a_once_volume_fires_for_the_first_actor_only() {
        let mut app = trigger_app();
        volume(&mut app, "cutscene", Vec3::ZERO, 2.0, true);
        actor(&mut app, Vec3::new(0.2, 0.0, 0.0));
        actor(&mut app, Vec3::new(-0.2, 0.0, 0.0));
        app.update();
        assert_eq!(
            entered(&mut app),
            vec!["cutscene".to_string()],
            "two actors, one cue"
        );
    }

    /// The invariant every reader depends on: each entry is closed by exactly
    /// one exit, `once` or not. A door that opens while occupied would
    /// otherwise leak open forever the first time it met a one-shot volume.
    #[test]
    fn a_once_volume_still_closes_its_one_entry() {
        let mut app = trigger_app();
        let volume_entity = volume(&mut app, "cutscene", Vec3::ZERO, 2.0, true);
        let walker = actor(&mut app, Vec3::ZERO);
        app.update();
        assert_eq!(entered(&mut app), vec!["cutscene".to_string()]);

        move_to(&mut app, walker, Vec3::new(9.0, 0.0, 0.0));
        app.update();
        assert_eq!(
            exited(&mut app),
            vec!["cutscene".to_string()],
            "walking out closes it"
        );
        assert!(
            app.world()
                .get::<TriggerState>(volume_entity)
                .unwrap()
                .spent
        );

        move_to(&mut app, walker, Vec3::ZERO);
        app.update();
        assert!(entered(&mut app).is_empty(), "and then it is over");
    }

    /// Two volumes over one spot are two volumes: no priority, no
    /// innermost-wins, no consuming the actor. Pinned because "it happens to
    /// work" and "it is the rule" are different things.
    #[test]
    fn overlapping_volumes_are_independent() {
        let mut app = trigger_app();
        volume(&mut app, "damage", Vec3::ZERO, 6.0, false);
        volume(&mut app, "music", Vec3::ZERO, 3.0, false);
        actor(&mut app, Vec3::new(30.0, 0.0, 0.0));
        app.update();
        let walker = app
            .world_mut()
            .query_filtered::<Entity, With<TriggerActor>>()
            .iter(app.world())
            .next()
            .unwrap();
        move_to(&mut app, walker, Vec3::ZERO);
        app.update();
        let mut fired = entered(&mut app);
        fired.sort();
        assert_eq!(fired, vec!["damage".to_string(), "music".to_string()]);
    }

    /// Editing is not play: a designer dragging the player through a kill
    /// volume must not kill the player.
    #[test]
    fn nothing_fires_while_the_editor_owns_the_world() {
        let mut app = trigger_app();
        app.world_mut().insert_resource(GameplayActive(false));
        volume(&mut app, "trigger", Vec3::ZERO, 2.0, false);
        actor(&mut app, Vec3::ZERO);
        app.update();
        app.update();
        assert!(entered(&mut app).is_empty());
        // Resuming notices the occupant that arrived while the editor held the
        // world — the edges are re-read against reality, not replayed.
        app.world_mut().insert_resource(GameplayActive(true));
        app.update();
        assert_eq!(entered(&mut app).len(), 1);
    }

    /// F6 pause freezes time without handing the world back to the editor.
    /// Nothing may fire under frozen time, or a paused inspection session
    /// trips the level.
    #[test]
    fn nothing_fires_while_time_is_paused() {
        let mut app = trigger_app();
        volume(&mut app, "trigger", Vec3::ZERO, 2.0, false);
        actor(&mut app, Vec3::ZERO);
        app.world_mut().resource_mut::<Time<Virtual>>().pause();
        app.update();
        app.update();
        assert!(entered(&mut app).is_empty());
    }
}
