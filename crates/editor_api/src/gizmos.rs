//! Custom gizmos (spec §7 designer surface): a game declares "this component
//! looks like THIS in the viewport" and gets a selectable, editable widget for
//! free — the editor needs zero knowledge of the type.
//!
//! Some of the most important things in a level have no geometry at all: a
//! player spawn, a trigger volume, a patrol node, a sound emitter. Without a
//! gizmo they are invisible; without a click target they are unselectable. Both
//! fall out of one registration.
//!
//! Drawing goes through `GizmoPainter` rather than handing out `Gizmos`
//! directly: the editor keeps ownership of the rendering plumbing, and a draw
//! function stays a plain `fn` that is trivial to unit test.

use bevy::prelude::*;

/// The primitives a gizmo can draw. Deliberately small — a gizmo is a
/// diagram, not a model.
pub trait GizmoPainter {
    fn line(&mut self, from: Vec3, to: Vec3, color: Color);
    fn sphere(&mut self, at: Vec3, radius: f32, color: Color);
    fn arrow(&mut self, from: Vec3, to: Vec3, color: Color);
    /// Wireframe box, oriented and scaled by `at`.
    fn cuboid(&mut self, at: Transform, color: Color);
    /// Circle facing `normal`.
    fn circle(&mut self, at: Vec3, normal: Vec3, radius: f32, color: Color);
}

/// What a draw function is given: where the entity is, whether it is selected,
/// and its component value for reading fields (a trigger's size, a light's
/// range) so the gizmo shows the DATA rather than a fixed glyph.
pub struct GizmoCx<'a> {
    pub painter: &'a mut dyn GizmoPainter,
    pub transform: GlobalTransform,
    pub selected: bool,
    pub value: &'a dyn bevy::reflect::PartialReflect,
}

impl GizmoCx<'_> {
    /// Convenience: the entity's world position.
    pub fn at(&self) -> Vec3 {
        self.transform.translation()
    }

    /// Convenience: a local direction in world space (gizmos are usually drawn
    /// along the entity's own axes).
    pub fn dir(&self, local: Vec3) -> Vec3 {
        self.transform.rotation() * local
    }

    /// The component, typed. `None` if the value is not the expected type.
    pub fn read<T: bevy::reflect::FromReflect>(&self) -> Option<T> {
        T::from_reflect(self.value)
    }
}

pub type GizmoDrawFn = fn(&mut GizmoCx);

/// One registered gizmo: which component it draws for, how to draw it, and how
/// big a click target the entity needs.
#[derive(Clone)]
pub struct GizmoDef {
    pub id: crate::ids::GizmoId,
    pub component: std::any::TypeId,
    pub draw: GizmoDrawFn,
    /// Radius of an invisible pick sphere placed on the entity, so a gizmo-only
    /// widget can be CLICKED in the viewport like anything with a mesh. `None`
    /// leaves it selectable from the hierarchy only.
    pub pick_radius: Option<f32>,
}
