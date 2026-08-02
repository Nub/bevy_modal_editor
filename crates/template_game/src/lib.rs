//! Library half of the reference game so integration tests (M1 acceptance A7) can
//! drive the same plugins the binary runs.

pub mod game;

#[cfg(feature = "editor")]
pub mod editor_overlay;
