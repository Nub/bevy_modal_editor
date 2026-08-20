//! The first track (spec §9 Animation, Layer 1 — the sequencer).
//!
//! Spec §9 describes a timeline whose tracks "keyframe any reflected component
//! property on any entity", which is the same addressing a patch already uses:
//! a type path plus a reflect path into it. A keyframe is that address plus a
//! value plus a time, so the substrate for this landed with `Op::Patch`.
//!
//! **Evaluation is not history.** Authoring a key is an edit and undoes like
//! one; moving the playhead is not, and writes straight to the component. A
//! scrub that pushed a transaction per frame would bury every real edit under
//! thousands of entries, and "undo" would start meaning "rewind time", which is
//! a different verb. The keys are the source of truth; what the playhead leaves
//! on screen is a view of them.

use bevy::prelude::*;
use bevy::reflect::ParsedPath;
use editor_api::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// How a segment LEAVES a key.
///
/// Linear motion reads as machinery: constant speed, instant starts, instant
/// stops. Easing is most of the difference between something that moves and
/// something that looks animated, and it costs one enum on a key.
///
/// The ease belongs to the key the segment STARTS from, so "this key eases out
/// into the next" is a property of the key you selected — which is the one you
/// are looking at when you decide.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
pub enum Ease {
    #[default]
    Linear,
    /// Starts slow. Weight taking up.
    In,
    /// Ends slow. Weight settling.
    Out,
    /// Both — the default shape of almost every deliberate movement.
    InOut,
    /// No interpolation at all: the value holds until the next key and then
    /// jumps. Switches, visibility, anything that has no in-between.
    Hold,
}

impl Ease {
    /// Reshape 0..1. Cubic: the cheapest curve that reads as ease rather than
    /// as a slightly bent line.
    pub fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        match self {
            Ease::Linear => t,
            Ease::In => t * t * t,
            Ease::Out => {
                let inverted = 1.0 - t;
                1.0 - inverted * inverted * inverted
            }
            Ease::InOut => {
                if t < 0.5 {
                    4.0 * t * t * t
                } else {
                    let inverted = -2.0 * t + 2.0;
                    1.0 - inverted * inverted * inverted / 2.0
                }
            }
            Ease::Hold => 0.0,
        }
    }

    /// Cycle for a control that has no room for five labels.
    pub fn next(self) -> Self {
        match self {
            Ease::Linear => Ease::InOut,
            Ease::InOut => Ease::In,
            Ease::In => Ease::Out,
            Ease::Out => Ease::Hold,
            Ease::Hold => Ease::Linear,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Ease::Linear => "linear",
            Ease::In => "ease in",
            Ease::Out => "ease out",
            Ease::InOut => "ease in-out",
            Ease::Hold => "hold",
        }
    }
}

/// A value at a time, and how it leaves for the next one.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Key {
    pub time: f32,
    pub value: f32,
    /// Defaulted, so every timeline written before easing existed still loads
    /// and behaves exactly as it did.
    pub ease: Ease,
}

impl Default for Key {
    fn default() -> Self {
        Self {
            time: 0.0,
            value: 0.0,
            ease: Ease::Linear,
        }
    }
}

/// One scalar field over time, addressed exactly as a patch addresses it.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Track {
    pub target: SceneId,
    pub type_path: String,
    pub path: String,
    /// Sorted by time; `set_key` maintains that.
    pub keys: Vec<Key>,
}

impl Track {
    /// The value at `time`: linear between the surrounding keys, held flat
    /// outside the first and last. Holding rather than extrapolating is what a
    /// designer expects — a platform with two keys waits at each end.
    pub fn sample(&self, time: f32) -> Option<f32> {
        let first = self.keys.first()?;
        if time <= first.time {
            return Some(first.value);
        }
        let last = self.keys.last()?;
        if time >= last.time {
            return Some(last.value);
        }
        let index = self.keys.iter().position(|key| key.time > time)?;
        let (before, after) = (self.keys[index - 1], self.keys[index]);
        let span = after.time - before.time;
        if span <= f32::EPSILON {
            return Some(after.value);
        }
        let t = before.ease.apply((time - before.time) / span);
        Some(before.value + (after.value - before.value) * t)
    }

    /// Set (or replace) the key at `time`. Replacing rather than appending is
    /// what makes keying twice at the same playhead an edit rather than a
    /// growing pile of coincident keys.
    pub fn set_key(&mut self, time: f32, value: f32) {
        match self
            .keys
            .iter()
            .position(|key| (key.time - time).abs() < 1e-4)
        {
            Some(index) => self.keys[index].value = value,
            None => {
                self.keys.push(Key {
                    time,
                    value,
                    ease: Ease::default(),
                });
                self.keys.sort_by(|a, b| a.time.total_cmp(&b.time));
            }
        }
    }
}

/// The authored tracks, in their own file beside the level.
///
/// Spec §9 calls this a timeline ASSET, and a sidecar is what that means here:
/// the material library already establishes the shape (own envelope, own format
/// version, atomic write with a backup), and keeping it out of the level's
/// hand-written serde means a timeline can gain fields without touching the
/// scene format at all.
#[derive(Resource)]
pub struct Timeline {
    pub tracks: Vec<Track>,
    pub events: Vec<EventMarker>,
    /// Bumped on every change; the autosave writes when it moves.
    pub generation: u64,
    pub path: PathBuf,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
            events: Vec::new(),
            generation: 0,
            path: PathBuf::from("timeline.ron"),
        }
    }
}

pub const TIMELINE_FORMAT_VERSION: u32 = 1;

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct TimelineEnvelope {
    format_version: u32,
    tracks: Vec<Track>,
    /// Added after format 1; `serde(default)` means a format-1 file still loads
    /// and simply has no events, which is what it meant.
    events: Vec<EventMarker>,
}

#[derive(Debug)]
pub enum TimelineError {
    Io(std::io::Error),
    Format(String),
    FutureVersion { found: u32, supported: u32 },
}

pub fn save_timeline(timeline: &Timeline, path: &Path) -> Result<(), TimelineError> {
    let envelope = TimelineEnvelope {
        format_version: TIMELINE_FORMAT_VERSION,
        tracks: timeline.tracks.clone(),
        events: timeline.events.clone(),
    };
    let text = ron::ser::to_string_pretty(&envelope, ron::ser::PrettyConfig::default())
        .map_err(|e| TimelineError::Format(e.to_string()))?;
    // Same atomic dance as the material library: write beside, back up, rename.
    let tmp = path.with_extension("ron.tmp");
    std::fs::write(&tmp, &text).map_err(TimelineError::Io)?;
    if path.exists() {
        let bak = path.with_extension("ron.bak");
        let _ = std::fs::copy(path, bak);
    }
    std::fs::rename(&tmp, path).map_err(TimelineError::Io)?;
    Ok(())
}

/// Parse fully before touching anything; a FUTURE version refuses loudly rather
/// than silently dropping tracks it does not understand.
pub fn load_timeline(path: &Path) -> Result<(Vec<Track>, Vec<EventMarker>), TimelineError> {
    let text = std::fs::read_to_string(path).map_err(TimelineError::Io)?;
    let envelope: TimelineEnvelope =
        ron::from_str(&text).map_err(|e| TimelineError::Format(e.to_string()))?;
    if envelope.format_version > TIMELINE_FORMAT_VERSION {
        return Err(TimelineError::FutureVersion {
            found: envelope.format_version,
            supported: TIMELINE_FORMAT_VERSION,
        });
    }
    Ok((envelope.tracks, envelope.events))
}

pub(crate) fn load_timeline_at_startup(mut timeline: ResMut<Timeline>) {
    let path = timeline.path.clone();
    match load_timeline(&path) {
        Ok((tracks, events)) => {
            if !tracks.is_empty() || !events.is_empty() {
                info!(
                    "timeline: loaded {} tracks, {} events",
                    tracks.len(),
                    events.len()
                );
            }
            timeline.tracks = tracks;
            timeline.events = events;
        }
        Err(TimelineError::Io(_)) => {} // no file yet is the normal first run
        Err(error) => error!("timeline load failed: {error:?}"),
    }
}

pub(crate) fn save_timeline_on_change(timeline: Res<Timeline>, mut last_saved: Local<u64>) {
    if timeline.generation == *last_saved || timeline.generation == 0 {
        return;
    }
    *last_saved = timeline.generation;
    let path = timeline.path.clone();
    if let Err(error) = save_timeline(&timeline, &path) {
        error!("timeline save failed: {error:?}");
    }
}

impl Timeline {
    /// The track for this address, created if it does not exist yet.
    pub fn track_mut(&mut self, target: SceneId, type_path: &str, path: &str) -> &mut Track {
        let existing = self.tracks.iter().position(|track| {
            track.target == target && track.type_path == type_path && track.path == path
        });
        let index = match existing {
            Some(index) => index,
            None => {
                self.tracks.push(Track {
                    target,
                    type_path: type_path.to_string(),
                    path: path.to_string(),
                    keys: Vec::new(),
                });
                self.tracks.len() - 1
            }
        };
        &mut self.tracks[index]
    }

    /// The last keyed moment — how long the thing actually runs for.
    pub fn duration(&self) -> f32 {
        self.tracks
            .iter()
            .filter_map(|track| track.keys.last().map(|key| key.time))
            .fold(0.0, f32::max)
    }
}

/// A named moment. Spec §9's second job for a sequencer: fire events at
/// timestamps — a footstep, a puff of dust, a trigger. The timeline says WHEN
/// and WHAT; the game decides what that means, which is why this carries a name
/// and nothing else.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct EventMarker {
    pub time: f32,
    pub name: String,
}

/// Fired as the playhead CROSSES a marker during playback. Games read this like
/// any other message.
#[derive(Message, Clone, Debug)]
pub struct TimelineEvent {
    pub name: String,
    pub time: f32,
}

/// Where time is, and whether it is moving.
#[derive(Resource, Default)]
pub struct Playhead {
    pub time: f32,
    pub playing: bool,
    /// Where time was when events were last fired. Crossing is a comparison
    /// between two moments, not a test of one.
    pub fired_through: f32,
}

/// Advance the playhead, looping at the end so a short loop reads as a loop
/// rather than stopping dead the first time round.
pub(crate) fn advance_playhead(
    // Optional: a headless test app has no TimePlugin, and a system that
    // demands a resource nobody registered takes the whole app down with it.
    time: Option<Res<Time>>,
    timeline: Res<Timeline>,
    mut playhead: ResMut<Playhead>,
) {
    if !playhead.playing {
        return;
    }
    let Some(time) = time else { return };
    let duration = timeline.duration();
    if duration <= 0.0 {
        return;
    }
    let next = playhead.time + time.delta_secs();
    playhead.time = if next > duration { 0.0 } else { next };
}

/// Which markers a step from `from` to `to` passes, given a looping timeline.
///
/// Half-open on the left — `(from, to]` — so a marker fires exactly once as it
/// is passed rather than once per frame while the playhead sits on it. A wrap
/// is two spans, because time went off the end and came back.
pub fn crossed(events: &[EventMarker], from: f32, to: f32, duration: f32) -> Vec<&EventMarker> {
    let passes = |a: f32, b: f32, marker: &EventMarker| marker.time > a && marker.time <= b;
    if to >= from {
        events
            .iter()
            .filter(|marker| passes(from, to, marker))
            .collect()
    } else {
        events
            .iter()
            .filter(|marker| passes(from, duration, marker) || passes(-1.0, to, marker))
            .collect()
    }
}

/// Fire the markers the playhead just passed. Playback only: scrubbing through
/// a footstep should not play forty footsteps, and dragging backwards should
/// not play them in reverse. Events belong to time RUNNING.
pub(crate) fn fire_timeline_events(
    timeline: Res<Timeline>,
    mut playhead: ResMut<Playhead>,
    mut events: MessageWriter<TimelineEvent>,
) {
    if !playhead.playing {
        // Keep the mark with the playhead so resuming does not fire everything
        // that was skipped while it was paused or scrubbed — but only WRITE
        // when it actually differs. Touching a ResMut every frame marks the
        // resource changed every frame, which made evaluation run constantly
        // and overwrite a pose before it could be keyed.
        if playhead.fired_through != playhead.time {
            playhead.fired_through = playhead.time;
        }
        return;
    }
    let duration = timeline.duration();
    if duration <= 0.0 {
        return;
    }
    let (from, to) = (playhead.fired_through, playhead.time);
    for marker in crossed(&timeline.events, from, to, duration) {
        events.write(TimelineEvent {
            name: marker.name.clone(),
            time: marker.time,
        });
    }
    playhead.fired_through = to;
}

/// Write the sampled value straight into the component. NOT through `EditScope`
/// — see the note at the top of this file.
pub(crate) fn evaluate_timeline(world: &mut World) {
    // Never fight the hands. Posing an object IS how a key gets authored, and a
    // track that already drives that field would otherwise snap it back in the
    // same frame the user moved it — you could never key a second pose.
    if !matches!(
        *world.resource::<editor_core::gesture::MoveGesture>(),
        editor_core::gesture::MoveGesture::Idle
    ) {
        return;
    }
    let (playing, moved) = {
        let playhead = world.resource_ref::<Playhead>();
        (playhead.playing, playhead.is_changed())
    };
    if !playing && !moved {
        return;
    }
    let at = world.resource::<Playhead>().time;
    let tracks: Vec<Track> = world.resource::<Timeline>().tracks.clone();
    if tracks.is_empty() {
        return;
    }
    let registry = world.resource::<AppTypeRegistry>().clone();
    let registry = registry.read();
    for track in tracks {
        let Some(value) = track.sample(at) else {
            continue;
        };
        let Some(entity) = world.resource::<SceneIndex>().get(&track.target) else {
            continue;
        };
        let Some(registration) = registry.get_with_type_path(&track.type_path) else {
            continue;
        };
        let Some(reflect_component) = registration.data::<bevy::ecs::reflect::ReflectComponent>()
        else {
            continue;
        };
        let Ok(parsed) = ParsedPath::parse(&track.path) else {
            continue;
        };
        let Ok(mut entity_mut) = world.get_entity_mut(entity) else {
            continue;
        };
        let Some(mut component) = reflect_component.reflect_mut(&mut entity_mut) else {
            continue;
        };
        if let Ok(element) = parsed.reflect_element_mut(component.as_partial_reflect_mut())
            && let Some(slot) = element.try_downcast_mut::<f32>()
        {
            *slot = value;
        }
    }
}

/// The transform path a key covers. Position only, deliberately: it is what a
/// prototype animates nine times in ten (a platform, a door, a lift), it needs
/// no rotation decomposition to be honest about, and a track is one SCALAR.
const KEYED_PATHS: [&str; 3] = ["translation.x", "translation.y", "translation.z"];

const TRANSFORM_PATH: &str = "bevy_transform::components::transform::Transform";

/// `anim.key` records where the selection IS at the playhead;
/// `anim.play` / `anim.stop` move time; `anim.rewind` puts it back to zero.
pub(crate) fn handle_anim_actions(
    mut reader: MessageReader<ActionInvoked>,
    mut timeline: ResMut<Timeline>,
    mut playhead: ResMut<Playhead>,
    selected: Query<(&SceneId, &Transform), With<editor_core::selection::Selected>>,
    mut feedback: MessageWriter<crate::SceneIoFeedback>,
) {
    for invoked in reader.read() {
        match invoked.action.as_str() {
            "anim.key" => {
                let at = playhead.time;
                let mut keyed = 0;
                for (id, transform) in selected.iter() {
                    for (index, path) in KEYED_PATHS.iter().enumerate() {
                        timeline
                            .track_mut(*id, TRANSFORM_PATH, path)
                            .set_key(at, transform.translation[index]);
                    }
                    keyed += 1;
                }
                if keyed > 0 {
                    // The autosave writes when this moves.
                    timeline.generation += 1;
                }
                let message = match keyed {
                    0 => "select something to key".to_string(),
                    1 => format!("keyed at {at:.2}s"),
                    n => format!("keyed {n} at {at:.2}s"),
                };
                feedback.write(crate::SceneIoFeedback {
                    message,
                    success: keyed > 0,
                });
            }
            "anim.play" => {
                if timeline.duration() <= 0.0 {
                    feedback.write(crate::SceneIoFeedback {
                        message: "nothing keyed yet".into(),
                        success: false,
                    });
                    continue;
                }
                playhead.playing = !playhead.playing;
                feedback.write(crate::SceneIoFeedback {
                    message: if playhead.playing {
                        "playing".into()
                    } else {
                        format!("paused at {:.2}s", playhead.time)
                    },
                    success: true,
                });
            }
            // Cycle the ease of every key sitting at the playhead. Keys at the
            // SAME moment move together because they were authored together —
            // the three axes of one pose are one decision, not three.
            "anim.ease" => {
                let at = playhead.time;
                let mut changed = 0;
                let mut label = Ease::Linear;
                for track in &mut timeline.tracks {
                    for key in &mut track.keys {
                        if (key.time - at).abs() < 1e-3 {
                            key.ease = key.ease.next();
                            label = key.ease;
                            changed += 1;
                        }
                    }
                }
                if changed > 0 {
                    timeline.generation += 1;
                }
                feedback.write(crate::SceneIoFeedback {
                    message: if changed > 0 {
                        format!("{} \u{2014} {}", label.label(), changed)
                    } else {
                        "no key at the playhead".into()
                    },
                    success: changed > 0,
                });
            }
            "anim.rewind" => {
                playhead.playing = false;
                playhead.time = 0.0;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track_with(keys: &[(f32, f32)]) -> Track {
        Track {
            target: SceneId::random(),
            type_path: "T".into(),
            path: "p".into(),
            keys: keys
                .iter()
                .map(|(time, value)| Key {
                    time: *time,
                    value: *value,
                    ease: Ease::Linear,
                })
                .collect(),
        }
    }

    fn markers(times: &[(f32, &str)]) -> Vec<EventMarker> {
        times
            .iter()
            .map(|(time, name)| EventMarker {
                time: *time,
                name: (*name).to_string(),
            })
            .collect()
    }

    fn names(found: Vec<&EventMarker>) -> Vec<String> {
        found.into_iter().map(|m| m.name.clone()).collect()
    }

    // A step passes what lies between its ends.
    #[test]
    fn a_step_crosses_the_markers_between_its_ends() {
        let events = markers(&[(0.5, "a"), (1.5, "b"), (2.5, "c")]);
        assert_eq!(names(crossed(&events, 1.0, 2.0, 3.0)), vec!["b"]);
        assert_eq!(names(crossed(&events, 0.0, 3.0, 3.0)), vec!["a", "b", "c"]);
        assert!(names(crossed(&events, 1.6, 2.4, 3.0)).is_empty());
    }

    // Half-open on the left: sitting ON a marker must not fire it again next
    // frame, or a held playhead machine-guns a footstep.
    #[test]
    fn a_marker_fires_once_as_it_is_passed() {
        let events = markers(&[(1.0, "step")]);
        assert_eq!(
            names(crossed(&events, 0.9, 1.0, 2.0)),
            vec!["step"],
            "passed"
        );
        assert!(
            names(crossed(&events, 1.0, 1.1, 2.0)).is_empty(),
            "and not again from there"
        );
    }

    // A loop is two spans: off the end, and back from the start.
    #[test]
    fn a_wrap_crosses_both_ends_of_the_loop() {
        let events = markers(&[(0.2, "start"), (1.0, "middle"), (1.9, "end")]);
        let found = names(crossed(&events, 1.8, 0.3, 2.0));
        assert!(
            found.contains(&"end".to_string()),
            "the one before the wrap"
        );
        assert!(found.contains(&"start".to_string()), "and after it");
        assert!(
            !found.contains(&"middle".to_string()),
            "but not the one the step never reached: {found:?}"
        );
    }

    // A marker exactly at zero is reachable — the wrap's second span is open at
    // its left end, so it has to include the very start.
    #[test]
    fn a_marker_at_the_start_is_reachable_on_a_wrap() {
        let events = markers(&[(0.0, "top")]);
        let found = names(crossed(&events, 1.9, 0.1, 2.0));
        assert_eq!(found, vec!["top"], "the loop point fires");
    }

    // Events persist with the tracks: a timeline is one asset.
    #[test]
    fn events_round_trip_with_the_timeline() {
        let dir = std::env::temp_dir().join(format!("timeline-ev-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("timeline.ron");
        let timeline = Timeline {
            path: path.clone(),
            events: markers(&[(0.4, "dust"), (1.2, "bang")]),
            ..Default::default()
        };
        save_timeline(&timeline, &path).unwrap();
        let (_tracks, events) = load_timeline(&path).unwrap();
        assert_eq!(events, timeline.events, "both markers came back");
        let _ = std::fs::remove_dir_all(&dir);
    }
    // A timeline is an ASSET: it has to come back the way it went out, or a
    // keyed animation is a session-long demo.
    #[test]
    fn a_timeline_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join(format!("timeline-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("timeline.ron");

        let mut timeline = Timeline {
            path: path.clone(),
            ..Default::default()
        };
        let target = SceneId::random();
        timeline
            .track_mut(target, TRANSFORM_PATH, "translation.y")
            .set_key(0.0, 1.0);
        timeline
            .track_mut(target, TRANSFORM_PATH, "translation.y")
            .set_key(2.0, 5.0);
        save_timeline(&timeline, &path).expect("saved");

        let (loaded, _events) = load_timeline(&path).expect("loaded");
        assert_eq!(loaded, timeline.tracks, "every track and key survived");
        assert_eq!(
            loaded[0].sample(1.0),
            Some(3.0),
            "and it still samples the same"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // A file from a LATER version refuses rather than silently dropping tracks
    // it does not understand — the same contract scenes and materials hold to.
    #[test]
    fn a_future_timeline_refuses_to_load() {
        let dir = std::env::temp_dir().join(format!("timeline-future-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("timeline.ron");
        std::fs::write(&path, "(format_version: 99, tracks: [])").unwrap();
        assert!(
            matches!(
                load_timeline(&path),
                Err(TimelineError::FutureVersion { found: 99, .. })
            ),
            "a future version is refused loudly"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // Saving is atomic and leaves a backup: a crash mid-write must not cost the
    // animation, and the previous version stays recoverable.
    #[test]
    fn saving_leaves_a_backup_and_no_temp_file() {
        let dir = std::env::temp_dir().join(format!("timeline-atomic-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("timeline.ron");
        let mut timeline = Timeline {
            path: path.clone(),
            ..Default::default()
        };
        timeline
            .track_mut(SceneId::random(), TRANSFORM_PATH, "translation.y")
            .set_key(0.0, 1.0);
        save_timeline(&timeline, &path).unwrap();
        timeline
            .track_mut(SceneId::random(), TRANSFORM_PATH, "translation.x")
            .set_key(0.0, 2.0);
        save_timeline(&timeline, &path).unwrap();

        assert!(path.exists(), "the file is there");
        assert!(
            path.with_extension("ron.bak").exists(),
            "and so is the previous version"
        );
        assert!(
            !path.with_extension("ron.tmp").exists(),
            "with no temp file left behind"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn eased_track(ease: Ease) -> Track {
        Track {
            target: SceneId::random(),
            type_path: "T".into(),
            path: "p".into(),
            keys: vec![
                Key {
                    time: 0.0,
                    value: 0.0,
                    ease,
                },
                Key {
                    time: 1.0,
                    value: 10.0,
                    ease: Ease::Linear,
                },
            ],
        }
    }

    // Every ease starts and ends where the keys say. A curve that misses its
    // own endpoints is not an ease, it is a bug with a nice name.
    #[test]
    fn every_ease_hits_both_keys_exactly() {
        for ease in [Ease::Linear, Ease::In, Ease::Out, Ease::InOut, Ease::Hold] {
            let track = eased_track(ease);
            assert_eq!(track.sample(0.0), Some(0.0), "{ease:?} starts at its key");
            assert_eq!(track.sample(1.0), Some(10.0), "{ease:?} ends at the next");
        }
    }

    // Ease IN starts slow: at halfway it has covered less than half the
    // distance. Ease OUT is the mirror.
    #[test]
    fn easing_in_starts_slow_and_out_ends_slow() {
        let midpoint = |ease| eased_track(ease).sample(0.5).unwrap();
        assert!(
            midpoint(Ease::In) < 5.0,
            "ease in lags at halfway: {}",
            midpoint(Ease::In)
        );
        assert!(
            midpoint(Ease::Out) > 5.0,
            "ease out leads at halfway: {}",
            midpoint(Ease::Out)
        );
        assert!(
            (midpoint(Ease::Linear) - 5.0).abs() < 1e-5,
            "and linear is exactly halfway"
        );
    }

    // Ease in-out is symmetric about the middle: what it gives up early it
    // gains back late, which is why it reads as deliberate.
    #[test]
    fn ease_in_out_is_symmetric() {
        let track = eased_track(Ease::InOut);
        assert!((track.sample(0.5).unwrap() - 5.0).abs() < 1e-4, "centred");
        let early = track.sample(0.25).unwrap();
        let late = track.sample(0.75).unwrap();
        assert!(
            ((10.0 - late) - early).abs() < 1e-4,
            "mirrored about the middle: {early} and {late}"
        );
    }

    // HOLD does not interpolate at all: the value stays put and then jumps,
    // which is what a switch or a visibility flag needs.
    #[test]
    fn hold_stays_put_until_the_next_key() {
        let track = eased_track(Ease::Hold);
        assert_eq!(track.sample(0.25), Some(0.0));
        assert_eq!(track.sample(0.99), Some(0.0), "still held");
        assert_eq!(track.sample(1.0), Some(10.0), "and then it is simply there");
    }

    // The ease belongs to the key the segment LEAVES, so a track can ease out
    // of one key and hold from the next.
    #[test]
    fn each_segment_uses_the_ease_of_the_key_it_leaves() {
        let track = Track {
            target: SceneId::random(),
            type_path: "T".into(),
            path: "p".into(),
            keys: vec![
                Key {
                    time: 0.0,
                    value: 0.0,
                    ease: Ease::Hold,
                },
                Key {
                    time: 1.0,
                    value: 10.0,
                    ease: Ease::Linear,
                },
                Key {
                    time: 2.0,
                    value: 20.0,
                    ease: Ease::Linear,
                },
            ],
        };
        assert_eq!(
            track.sample(0.5),
            Some(0.0),
            "held across the first segment"
        );
        assert_eq!(
            track.sample(1.5),
            Some(15.0),
            "and linear across the second"
        );
    }

    // Cycling reaches every mode and comes back — a control with no room for
    // five labels still has to be able to select all five.
    #[test]
    fn cycling_ease_visits_every_mode() {
        let mut seen = vec![Ease::Linear];
        let mut current = Ease::Linear;
        for _ in 0..4 {
            current = current.next();
            seen.push(current);
        }
        assert_eq!(current.next(), Ease::Linear, "and wraps");
        for ease in [Ease::Linear, Ease::In, Ease::Out, Ease::InOut, Ease::Hold] {
            assert!(seen.contains(&ease), "{ease:?} is reachable");
        }
    }
    // Between two keys the value moves; the halfway point is halfway.
    #[test]
    fn a_track_interpolates_between_its_keys() {
        let track = track_with(&[(0.0, 0.0), (2.0, 10.0)]);
        assert_eq!(track.sample(0.0), Some(0.0));
        assert_eq!(track.sample(1.0), Some(5.0));
        assert_eq!(track.sample(2.0), Some(10.0));
        assert_eq!(track.sample(0.5), Some(2.5));
    }

    // Outside the keyed range it HOLDS. A platform with two keys waits at each
    // end; extrapolating would send it off into space.
    #[test]
    fn a_track_holds_outside_its_keys() {
        let track = track_with(&[(1.0, 4.0), (2.0, 8.0)]);
        assert_eq!(track.sample(0.0), Some(4.0), "before the first key");
        assert_eq!(track.sample(9.0), Some(8.0), "after the last");
    }

    #[test]
    fn an_empty_track_samples_to_nothing() {
        assert_eq!(track_with(&[]).sample(1.0), None);
    }

    // Keying twice at the same moment REPLACES: otherwise adjusting a pose
    // leaves two keys at one time and the later one silently wins.
    #[test]
    fn keying_the_same_moment_replaces_it() {
        let mut track = track_with(&[(0.0, 1.0)]);
        track.set_key(0.0, 7.0);
        assert_eq!(track.keys.len(), 1, "still one key");
        assert_eq!(track.keys[0].value, 7.0, "carrying the new value");
    }

    // Keys stay sorted however they are authored — sampling walks them in order.
    #[test]
    fn keys_stay_in_time_order() {
        let mut track = track_with(&[]);
        for (time, value) in [(2.0, 20.0), (0.0, 0.0), (1.0, 10.0)] {
            track.set_key(time, value);
        }
        let times: Vec<f32> = track.keys.iter().map(|key| key.time).collect();
        assert_eq!(times, vec![0.0, 1.0, 2.0]);
        assert_eq!(track.sample(0.5), Some(5.0), "and interpolate correctly");
    }

    // The timeline runs as long as its last key, which is what the playhead
    // loops on.
    #[test]
    fn the_duration_is_the_last_keyed_moment() {
        let mut timeline = Timeline::default();
        let target = SceneId::random();
        timeline.track_mut(target, "T", "a").set_key(1.5, 0.0);
        timeline.track_mut(target, "T", "b").set_key(4.0, 0.0);
        assert_eq!(timeline.duration(), 4.0);
    }

    // One track per address: keying the same field twice must not fork it.
    #[test]
    fn one_address_is_one_track() {
        let mut timeline = Timeline::default();
        let target = SceneId::random();
        timeline
            .track_mut(target, "T", "translation.y")
            .set_key(0.0, 1.0);
        timeline
            .track_mut(target, "T", "translation.y")
            .set_key(1.0, 2.0);
        assert_eq!(timeline.tracks.len(), 1, "one track");
        assert_eq!(timeline.tracks[0].keys.len(), 2, "with both keys");
    }
}
