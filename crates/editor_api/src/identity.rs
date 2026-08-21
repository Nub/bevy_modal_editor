//! What makes two objects "the same thing" — declared as DATA by the crate that
//! owns the component (spec §9, the `*` verb in docs/spec/03-KEYMAP-DESIGN.md).
//!
//! The kernel has to answer "select every one like this" without knowing what a
//! prefab, a model or a game primitive is: `editor_core` depends on `bevy`,
//! `editor_api`, `serde` and `ron`, and nothing else. So identity is a
//! registered ladder of reflected comparisons, exactly the way entity kinds and
//! gizmos already are. `editor_prefabs` says "same prefab means the same
//! `PrefabInstance` uuid"; a game says "same primitive means the same `kind`
//! field"; the kernel compares reflected values and never names either.
//!
//! Registration mistakes are startup panics, not a verb that quietly does
//! nothing (spec §8): a rung whose key stops resolving is a `*` that stops
//! working, and silence is how that ships.

/// One rung of the identity ladder.
#[derive(Clone, Debug)]
pub struct IdentityDef {
    /// Lower wins. The FIRST rung whose component is present decides — a
    /// barrel that is both a prefab instance and carries a mesh is a barrel.
    pub priority: u32,
    pub component: std::any::TypeId,
    pub type_path: &'static str,
    /// What part of the component is the identity:
    /// - `""` — the WHOLE component value.
    /// - `"*"` — PRESENCE only; the value is irrelevant.
    /// - otherwise — one named struct field, compared on its own.
    ///
    /// The middle case exists for components whose value is per-object by
    /// construction: two trigger volumes are the same kind of thing even
    /// though one is named "lift" and the other "pit".
    pub key: &'static str,
    /// What the feedback calls this family: "same prefab", "same model".
    pub noun: &'static str,
}

/// Priority bands, so features can slot in without knowing each other's numbers.
pub mod priority {
    /// An instance of a prefab is that prefab, whatever else it carries.
    pub const PREFAB: u32 = 100;
    /// A placed import: same source asset.
    pub const MODEL: u32 = 200;
    /// A materialized node inside an import.
    pub const MESH_NODE: u32 = 300;
    /// Game-declared kinds, below everything the editor knows about.
    pub const GAME: u32 = 500;
}
