//! Spline sampling for procedural placement.
//!
//! This module is only available with the "spline" feature.

use bevy::prelude::*;
use bevy_spline_3d::spline::{ArcLengthTable, Spline, DEFAULT_ARC_LENGTH_SAMPLES};

use crate::sampling::{Sample, SampleOrientation};

/// Sample a spline for procedural placement.
pub fn sample_spline(
    spline: &Spline,
    count: usize,
    uniform: bool,
    seed: Option<u64>,
) -> Vec<Sample> {
    if count == 0 || !spline.is_valid() {
        return Vec::new();
    }

    if uniform {
        sample_spline_uniform(spline, count)
    } else {
        sample_spline_random(spline, count, seed)
    }
}

fn sample_spline_uniform(spline: &Spline, count: usize) -> Vec<Sample> {
    // Use arc-length parameterization for uniform spacing
    let table = ArcLengthTable::compute(spline, DEFAULT_ARC_LENGTH_SAMPLES);
    let t_values = table.uniform_t_values(count);

    t_values
        .into_iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let position = spline.evaluate(t)?;
            let tangent = spline.evaluate_tangent(t).map(|v| v.normalize_or_zero());

            Some(Sample {
                position,
                orientation: SampleOrientation {
                    tangent,
                    up: Some(Vec3::Y),
                },
                parameter: if count > 1 {
                    i as f32 / (count - 1) as f32
                } else {
                    0.5
                },
            })
        })
        .collect()
}

fn sample_spline_random(spline: &Spline, count: usize, seed: Option<u64>) -> Vec<Sample> {
    let mut rng = if let Some(s) = seed {
        fastrand::Rng::with_seed(s)
    } else {
        fastrand::Rng::new()
    };

    (0..count)
        .filter_map(|i| {
            let t = rng.f32();
            let position = spline.evaluate(t)?;
            let tangent = spline.evaluate_tangent(t).map(|v| v.normalize_or_zero());

            Some(Sample {
                position,
                orientation: SampleOrientation {
                    tangent,
                    up: Some(Vec3::Y),
                },
                parameter: if count > 1 {
                    i as f32 / (count - 1) as f32
                } else {
                    0.5
                },
            })
        })
        .collect()
}
