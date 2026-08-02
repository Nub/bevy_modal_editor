//! Stable identifiers (RFC §2). String IDs are kebab-case, dot-namespaced by feature
//! (`"splines.point.add"`); cheap to clone, order-stable, and data-file friendly.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt;
use uuid::Uuid;

macro_rules! string_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        pub struct $name(Cow<'static, str>);

        impl $name {
            pub const fn new_static(s: &'static str) -> Self {
                Self(Cow::Borrowed(s))
            }
            pub fn new(s: impl Into<String>) -> Self {
                Self(Cow::Owned(s.into()))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl fmt::Debug for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, concat!(stringify!($name), "({})"), self.0)
            }
        }
        impl From<&'static str> for $name {
            fn from(s: &'static str) -> Self {
                Self(Cow::Borrowed(s))
            }
        }
    };
}

string_id!(
    /// A registered feature crate ("splines", "vfx").
    FeatureId
);
string_id!(
    /// A named editor action ("splines.point.add").
    ActionId
);
string_id!(
    /// A keymap layer: every mode and focused panel is one; "global" always active.
    ContextId
);
string_id!(
    /// A modal editor mode ("normal", "insert", "spline-edit").
    ModeId
);
string_id!(
    /// A registered panel.
    PanelId
);
string_id!(
    /// A spawnable entity kind ("splines.catmull-rom").
    EntityKindId
);

/// The always-active keymap context.
pub const GLOBAL_CONTEXT: ContextId = ContextId::new_static("global");

/// Stable scene-entity identity (spec §5): all editor references target `SceneId`,
/// never `Entity`. `Default + Clone` keeps it BSN-blanket-template compatible
/// (proven in M0 spike 2).
#[derive(
    Component, Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize,
)]
pub struct SceneId(pub Uuid);

impl SceneId {
    pub fn random() -> Self {
        Self(Uuid::new_v4())
    }
}
