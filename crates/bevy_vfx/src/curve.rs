//! Keyframed curves and color gradients for VFX parameter animation.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Curve<T> — keyframed value over normalized time [0..1]
// ---------------------------------------------------------------------------

/// A keyframed curve mapping normalized time [0..1] to a value.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub struct Curve<T: Clone + PartialEq + Reflect> {
    pub keys: Vec<CurveKey<T>>,
}

/// Single keyframe in a curve.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub struct CurveKey<T: Clone + PartialEq + Reflect> {
    /// Normalized time (0.0 - 1.0).
    pub time: f32,
    /// Value at this keyframe.
    pub value: T,
    /// Interpolation mode to the next key.
    pub interp: Interp,
}

/// Interpolation mode between keyframes.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Reflect)]
pub enum Interp {
    #[default]
    Linear,
    EaseIn,
    EaseOut,
    EaseInOut,
    Constant,
}

impl Interp {
    /// Apply easing to a linear factor `t` in [0..1].
    pub fn ease(&self, t: f32) -> f32 {
        match self {
            Self::Linear => t,
            Self::EaseIn => t * t,
            Self::EaseOut => 1.0 - (1.0 - t) * (1.0 - t),
            Self::EaseInOut => {
                if t < 0.5 {
                    2.0 * t * t
                } else {
                    1.0 - (-2.0 * t + 2.0).powi(2) / 2.0
                }
            }
            Self::Constant => 0.0, // Hold previous value
        }
    }
}

// -- f32 curve utilities --

impl Curve<f32> {
    /// Create a constant curve (single key).
    pub fn constant(value: f32) -> Self {
        Self {
            keys: vec![CurveKey {
                time: 0.0,
                value,
                interp: Interp::Linear,
            }],
        }
    }

    /// Create a linear ramp from `start` to `end`.
    pub fn linear(start: f32, end: f32) -> Self {
        Self {
            keys: vec![
                CurveKey {
                    time: 0.0,
                    value: start,
                    interp: Interp::Linear,
                },
                CurveKey {
                    time: 1.0,
                    value: end,
                    interp: Interp::Linear,
                },
            ],
        }
    }

    /// Sample the curve at normalized time `t` (clamped to [0..1]).
    pub fn sample(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);

        if self.keys.is_empty() {
            return 0.0;
        }
        if self.keys.len() == 1 {
            return self.keys[0].value;
        }

        // Find the two keys surrounding `t`
        if t <= self.keys[0].time {
            return self.keys[0].value;
        }
        if t >= self.keys.last().unwrap().time {
            return self.keys.last().unwrap().value;
        }

        for window in self.keys.windows(2) {
            let (a, b) = (&window[0], &window[1]);
            if t >= a.time && t <= b.time {
                let span = b.time - a.time;
                if span.abs() < 1e-6 {
                    return a.value;
                }
                let frac = (t - a.time) / span;
                let eased = a.interp.ease(frac);
                return a.value + (b.value - a.value) * eased;
            }
        }

        self.keys.last().unwrap().value
    }

    /// Pack curve into a flat array of (time, value) pairs for GPU upload.
    /// Returns up to `max_keys` pairs, linearly resampled if the curve has more.
    pub fn pack_for_gpu(&self, max_keys: usize) -> Vec<[f32; 2]> {
        if self.keys.is_empty() {
            return vec![[0.0, 0.0]];
        }

        if self.keys.len() <= max_keys {
            return self.keys.iter().map(|k| [k.time, k.value]).collect();
        }

        // Resample to max_keys evenly-spaced points
        (0..max_keys)
            .map(|i| {
                let t = i as f32 / (max_keys - 1).max(1) as f32;
                [t, self.sample(t)]
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Gradient — color over normalized time [0..1]
// ---------------------------------------------------------------------------

/// A color gradient mapping normalized time [0..1] to an RGBA color.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub struct Gradient {
    pub keys: Vec<GradientKey>,
}

/// Single color stop in a gradient.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Reflect)]
pub struct GradientKey {
    /// Normalized time (0.0 - 1.0).
    pub time: f32,
    /// RGBA color at this stop.
    pub color: LinearRgba,
}

impl Gradient {
    /// Create a gradient that goes from white to transparent white.
    pub fn white_to_transparent() -> Self {
        Self {
            keys: vec![
                GradientKey {
                    time: 0.0,
                    color: LinearRgba::WHITE,
                },
                GradientKey {
                    time: 1.0,
                    color: LinearRgba::new(1.0, 1.0, 1.0, 0.0),
                },
            ],
        }
    }

    /// Create a constant color gradient.
    pub fn constant(color: LinearRgba) -> Self {
        Self {
            keys: vec![GradientKey { time: 0.0, color }],
        }
    }

    /// Sample the gradient at normalized time `t` (clamped to [0..1]).
    pub fn sample(&self, t: f32) -> LinearRgba {
        let t = t.clamp(0.0, 1.0);

        if self.keys.is_empty() {
            return LinearRgba::WHITE;
        }
        if self.keys.len() == 1 {
            return self.keys[0].color;
        }

        if t <= self.keys[0].time {
            return self.keys[0].color;
        }
        if t >= self.keys.last().unwrap().time {
            return self.keys.last().unwrap().color;
        }

        for window in self.keys.windows(2) {
            let (a, b) = (&window[0], &window[1]);
            if t >= a.time && t <= b.time {
                let span = b.time - a.time;
                if span.abs() < 1e-6 {
                    return a.color;
                }
                let frac = (t - a.time) / span;
                return lerp_color(a.color, b.color, frac);
            }
        }

        self.keys.last().unwrap().color
    }

    /// Pack gradient into a flat array of (time, r, g, b, a) for GPU upload.
    pub fn pack_for_gpu(&self, max_keys: usize) -> Vec<[f32; 5]> {
        if self.keys.is_empty() {
            return vec![[0.0, 1.0, 1.0, 1.0, 1.0]];
        }

        if self.keys.len() <= max_keys {
            return self
                .keys
                .iter()
                .map(|k| {
                    [
                        k.time, k.color.red, k.color.green, k.color.blue, k.color.alpha,
                    ]
                })
                .collect();
        }

        // Resample
        (0..max_keys)
            .map(|i| {
                let t = i as f32 / (max_keys - 1).max(1) as f32;
                let c = self.sample(t);
                [t, c.red, c.green, c.blue, c.alpha]
            })
            .collect()
    }
}

impl Default for Gradient {
    fn default() -> Self {
        Self::white_to_transparent()
    }
}

/// Linearly interpolate between two colors.
fn lerp_color(a: LinearRgba, b: LinearRgba, t: f32) -> LinearRgba {
    LinearRgba::new(
        a.red + (b.red - a.red) * t,
        a.green + (b.green - a.green) * t,
        a.blue + (b.blue - a.blue) * t,
        a.alpha + (b.alpha - a.alpha) * t,
    )
}
