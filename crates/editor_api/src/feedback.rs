//! The editor's one user-facing feedback channel.
//!
//! It lived in `editor_scene` because save/load was the first thing that had
//! something to say, and its name still shows that. It belongs HERE: the
//! kernel refuses edits (a locked object), the prefab layer refuses scene I/O,
//! ingest reports what it cooked — every layer of the editor needs to speak,
//! and a crate cannot speak through a channel defined above it.
//!
//! Spec §8, design bar: logging is not user feedback. A verb that does nothing
//! and says nothing reads as a broken editor.

use bevy::prelude::*;

/// A transient, user-facing result. The statusbar flashes it (success in the
/// content tier, failure in the warn tone) — see `editor_ui::statusbar`.
#[derive(Message, Debug)]
pub struct SceneIoFeedback {
    pub message: String,
    pub success: bool,
}
