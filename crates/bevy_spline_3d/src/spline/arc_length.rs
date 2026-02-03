//! Arc length parameterization utilities for splines.
//!
//! Provides functions to compute arc length and convert between arc length
//! and t-parameter space for uniform spacing along splines.

use bevy::prelude::*;

use super::Spline;

/// Default number of samples for arc length calculations.
pub const DEFAULT_ARC_LENGTH_SAMPLES: usize = 128;

/// Arc length lookup table for efficient t-to-length and length-to-t conversion.
///
/// The table is built by sampling the spline at regular t intervals and
/// accumulating the distance between samples.
#[derive(Debug, Clone)]
pub struct ArcLengthTable {
    /// (t, cumulative_length) pairs, always starting with (0.0, 0.0).
    samples: Vec<(f32, f32)>,
}

impl ArcLengthTable {
    /// Compute an arc length table for a spline.
    ///
    /// # Arguments
    /// * `spline` - The spline to sample
    /// * `samples` - Number of samples (more = higher accuracy)
    pub fn compute(spline: &Spline, samples: usize) -> Self {
        let mut table = Vec::with_capacity(samples + 1);
        let mut cumulative_length = 0.0;
        let mut prev_point = spline.evaluate(0.0).unwrap_or(Vec3::ZERO);

        table.push((0.0, 0.0));

        for i in 1..=samples {
            let t = i as f32 / samples as f32;
            let point = spline.evaluate(t).unwrap_or(prev_point);
            cumulative_length += (point - prev_point).length();
            table.push((t, cumulative_length));
            prev_point = point;
        }

        Self { samples: table }
    }

    /// Get the total arc length of the spline.
    pub fn total_length(&self) -> f32 {
        self.samples.last().map(|(_, l)| *l).unwrap_or(0.0)
    }

    /// Find the t parameter for a given arc length.
    ///
    /// Returns a value in [0, 1] corresponding to the position along
    /// the spline at the given arc length from the start.
    pub fn length_to_t(&self, target_length: f32) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }

        let total_length = self.total_length();
        if total_length <= 0.0 {
            return 0.0;
        }

        // Clamp target length
        let target = target_length.clamp(0.0, total_length);

        // Binary search for the segment containing target_length
        let idx = self
            .samples
            .binary_search_by(|(_, l)| l.partial_cmp(&target).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or_else(|i| i.saturating_sub(1));

        if idx >= self.samples.len() - 1 {
            return 1.0;
        }

        let (t0, l0) = self.samples[idx];
        let (t1, l1) = self.samples[idx + 1];

        if (l1 - l0).abs() < 1e-6 {
            return t0;
        }

        // Linear interpolation within segment
        let alpha = (target - l0) / (l1 - l0);
        t0 + alpha * (t1 - t0)
    }

    /// Get the arc length at a given t parameter.
    ///
    /// This interpolates between samples for smooth results.
    pub fn t_to_length(&self, t: f32) -> f32 {
        if self.samples.is_empty() {
            return 0.0;
        }

        let t = t.clamp(0.0, 1.0);

        // Find the bracketing samples
        let samples = self.samples.len();
        let float_idx = t * (samples - 1) as f32;
        let idx = (float_idx as usize).min(samples - 2);

        let (t0, l0) = self.samples[idx];
        let (t1, l1) = self.samples[idx + 1];

        if (t1 - t0).abs() < 1e-6 {
            return l0;
        }

        // Linear interpolation
        let alpha = (t - t0) / (t1 - t0);
        l0 + alpha * (l1 - l0)
    }

    /// Compute t values for uniform spacing along the arc length.
    ///
    /// Returns `count` evenly-spaced t values that correspond to
    /// uniform distances along the spline.
    pub fn uniform_t_values(&self, count: usize) -> Vec<f32> {
        if count == 0 {
            return Vec::new();
        }
        if count == 1 {
            return vec![0.5];
        }

        let total = self.total_length();
        if total <= 0.0 {
            // Fall back to uniform t distribution
            return (0..count).map(|i| i as f32 / (count - 1) as f32).collect();
        }

        (0..count)
            .map(|i| {
                let target_length = total * i as f32 / (count - 1) as f32;
                self.length_to_t(target_length)
            })
            .collect()
    }
}

/// Approximate the total arc length of a spline without building a table.
///
/// This is more efficient when you only need the total length, not
/// individual position lookups.
pub fn approximate_arc_length(spline: &Spline, samples: usize) -> f32 {
    let mut length = 0.0;
    let mut prev_point = spline.evaluate(0.0).unwrap_or(Vec3::ZERO);

    for i in 1..=samples {
        let t = i as f32 / samples as f32;
        let point = spline.evaluate(t).unwrap_or(prev_point);
        length += (point - prev_point).length();
        prev_point = point;
    }

    length
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spline::SplineType;

    #[test]
    fn test_arc_length_endpoints() {
        let spline = Spline::new(
            SplineType::CatmullRom,
            vec![
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(1.0, 0.0, 0.0),
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(3.0, 0.0, 0.0),
            ],
        );

        let table = ArcLengthTable::compute(&spline, 100);

        // t=0 should give length=0
        assert!((table.t_to_length(0.0) - 0.0).abs() < 0.01);

        // length=0 should give t=0
        assert!((table.length_to_t(0.0) - 0.0).abs() < 0.01);

        // t=1 should give total length
        assert!((table.t_to_length(1.0) - table.total_length()).abs() < 0.01);

        // total length should give t=1
        assert!((table.length_to_t(table.total_length()) - 1.0).abs() < 0.01);
    }
}
