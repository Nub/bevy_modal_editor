//! Light gizmos (owner ask): a light is invisible — the thing you place has no
//! geometry, and its PARAMETERS (how far it reaches, which way it points, how
//! wide the cone opens) are pure numbers. Drawn in the light's own colour, so
//! the gizmo reads as the light it belongs to rather than generic chrome.
//!
//! Immediate-mode gizmos, editor-only: nothing is spawned, nothing serializes,
//! and the game never sees them.

use bevy::prelude::*;
use editor_core::prelude::*;

/// Selected lights draw solid; unselected ones dim, so a scene full of lights
/// stays readable while the one you are editing stands out.
fn tint(color: Color, selected: bool) -> Color {
    color.with_alpha(if selected { 0.9 } else { 0.35 })
}

/// The three shapes, each showing what you can actually tune:
/// point = reach, spot = reach + cone, directional = direction.
pub(crate) fn draw_light_gizmos(
    state: Res<EditorState>,
    mut gizmos: Gizmos,
    // `InheritedVisibility` because these are immediate-mode: hiding a light
    // removes its mesh, never its gizmo, and a range sphere floating over a
    // hidden lamp is worse than no gizmo at all. Both light types
    // `#[require(Visibility)]`, so the term is always present and true.
    points: Query<(
        &GlobalTransform,
        &PointLight,
        Has<Selected>,
        &InheritedVisibility,
    )>,
    spots: Query<(
        &GlobalTransform,
        &SpotLight,
        Has<Selected>,
        &InheritedVisibility,
    )>,
    directionals: Query<(&GlobalTransform, &DirectionalLight, Has<Selected>)>,
) {
    if !state.active {
        return;
    }
    for (global, light, selected, visible) in &points {
        if !visible.get() {
            continue;
        }
        let at = global.translation();
        let color = tint(light.color, selected);
        // Range is the falloff cutoff — the volume the light can affect.
        gizmos.sphere(at, light.range, color);
        // A small cross marks the source itself, which the range sphere hides
        // at any distance.
        for axis in [Vec3::X, Vec3::Y, Vec3::Z] {
            gizmos.line(at - axis * 0.15, at + axis * 0.15, color);
        }
    }
    for (global, light, selected, visible) in &spots {
        if !visible.get() {
            continue;
        }
        let at = global.translation();
        let color = tint(light.color, selected);
        // Bevy spot lights shine down -Z.
        let direction = global.rotation() * -Vec3::Z;
        let tip = at + direction * light.range;
        gizmos.line(at, tip, color);
        // Outer angle is the half-angle of the cone: the radius it covers at
        // range is what a designer is actually aiming.
        let outer = light.outer_angle.tan() * light.range;
        let inner = light.inner_angle.tan() * light.range;
        let facing = Quat::from_rotation_arc(Vec3::Z, direction);
        gizmos.circle(Isometry3d::new(tip, facing), outer, color);
        if inner > 0.0 && inner < outer {
            gizmos.circle(Isometry3d::new(tip, facing), inner, color.with_alpha(0.25));
        }
        // Four rays sketch the cone surface between apex and rim.
        let right = facing * Vec3::X;
        let up = facing * Vec3::Y;
        for edge in [right, -right, up, -up] {
            gizmos.line(at, tip + edge * outer, color);
        }
    }
    for (global, light, selected) in &directionals {
        let at = global.translation();
        let color = tint(light.color, selected);
        let direction = global.rotation() * -Vec3::Z;
        // Direction is the ONLY spatial parameter — position is irrelevant to
        // the shading, so the gizmo is a bundle of parallel rays saying so.
        let facing = Quat::from_rotation_arc(Vec3::Z, direction);
        gizmos.circle(Isometry3d::new(at, facing), 0.5, color);
        for offset in [
            Vec3::ZERO,
            Vec3::X * 0.5,
            -Vec3::X * 0.5,
            Vec3::Y * 0.5,
            -Vec3::Y * 0.5,
        ] {
            let start = at + facing * offset;
            gizmos.arrow(start, start + direction * 1.5, color);
        }
    }
}
