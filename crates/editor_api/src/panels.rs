//! Panels (RFC §9): declarative registration. A panel is DATA — id, title, placement,
//! and the keymap context active while it holds focus. The layout manager (editor_ui)
//! owns docking, chrome, and focus; a panel can never draw its own window chrome.
//!
//! Panel focus is a *focus target with its own keymap layer* (spec §"Modes"), not a
//! mode: while focused, the panel's context replaces the mode layer (j/k belong to the
//! tree, not the viewport) and Escape returns focus to the viewport.

use crate::ids::{ContextId, PanelId};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Placement {
    Left,
    Right,
    Bottom,
}

/// What a `Properties` panel reflects over.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PropertySource {
    /// The current selection (the inspector).
    Selection,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PanelContent {
    /// Zero-UI-code path: the reflection editor over a source. Most panels
    /// should be this (RFC §9).
    Properties(PropertySource),
    /// The feature registers a custom renderer with `editor_ui` keyed by `PanelId`.
    Custom,
}

#[derive(Clone, Debug)]
pub struct PanelDecl {
    pub id: PanelId,
    pub title: &'static str,
    pub placement: Placement,
    /// Keymap layer while this panel is focused. Registered implicitly.
    pub context: ContextId,
    pub content: PanelContent,
    /// Whether the panel starts open.
    pub default_open: bool,
}
