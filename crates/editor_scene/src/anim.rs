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

/// A value at a time.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct Key {
    pub time: f32,
    pub value: f32,
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
        let t = (time - before.time) / span;
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
                self.keys.push(Key { time, value });
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
    /// Bumped on every change; the autosave writes when it moves.
    pub generation: u64,
    pub path: PathBuf,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            tracks: Vec::new(),
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
pub fn load_timeline(path: &Path) -> Result<Vec<Track>, TimelineError> {
    let text = std::fs::read_to_string(path).map_err(TimelineError::Io)?;
    let envelope: TimelineEnvelope =
        ron::from_str(&text).map_err(|e| TimelineError::Format(e.to_string()))?;
    if envelope.format_version > TIMELINE_FORMAT_VERSION {
        return Err(TimelineError::FutureVersion {
            found: envelope.format_version,
            supported: TIMELINE_FORMAT_VERSION,
        });
    }
    Ok(envelope.tracks)
}

pub(crate) fn load_timeline_at_startup(mut timeline: ResMut<Timeline>) {
    let path = timeline.path.clone();
    match load_timeline(&path) {
        Ok(tracks) => {
            if !tracks.is_empty() {
                info!("timeline: loaded {} tracks", tracks.len());
            }
            timeline.tracks = tracks;
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

/// Where time is, and whether it is moving.
#[derive(Resource, Default)]
pub struct Playhead {
    pub time: f32,
    pub playing: bool,
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
                })
                .collect(),
        }
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

        let loaded = load_timeline(&path).expect("loaded");
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
