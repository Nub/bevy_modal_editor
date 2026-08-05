//! Appear micro-animation (design bar: award-level polish): floating surfaces
//! ease in — scale 0.97→1, lift −6px→0 — over 120ms with cubic ease-out.
//! One marker (`FloatingSurface`) on every popup root; visibility flips arm it.

use bevy::prelude::*;
use bevy::ui::UiTransform;
use bevy::ui::Val2;

/// Every floating popup root carries this; the animation runs each time the
/// surface becomes visible.
#[derive(Component, Default, Clone)]
pub(crate) struct FloatingSurface {
    /// Animation clock; None = at rest.
    t: Option<f32>,
    was_visible: bool,
}

const DURATION: f32 = 0.12;

pub(crate) fn animate_appearing(
    time: Res<Time>,
    mut surfaces: Query<(&mut FloatingSurface, &Visibility, &mut UiTransform)>,
) {
    for (mut surface, visibility, mut transform) in &mut surfaces {
        let visible = *visibility == Visibility::Visible;
        if visible && !surface.was_visible {
            surface.t = Some(0.0);
        }
        surface.was_visible = visible;
        let Some(t) = surface.t else {
            continue;
        };
        let t = (t + time.delta_secs() / DURATION).min(1.0);
        // Cubic ease-out: fast start, soft landing.
        let eased = 1.0 - (1.0 - t).powi(3);
        transform.scale = Vec2::splat(0.97 + 0.03 * eased);
        transform.translation = Val2::px(0.0, -6.0 * (1.0 - eased));
        surface.t = (t < 1.0).then_some(t);
        if t >= 1.0 {
            *transform = UiTransform::IDENTITY;
        }
    }
}
