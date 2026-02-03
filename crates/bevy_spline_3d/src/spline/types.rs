use bevy::prelude::*;

/// The type of spline interpolation to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Reflect)]
#[reflect(Default)]
pub enum SplineType {
    /// Cubic Bézier spline - 4 control points per segment.
    /// Points 0 and 3 are on the curve, 1 and 2 are handles.
    #[default]
    CubicBezier,
    /// Catmull-Rom spline - passes through all control points.
    /// Requires at least 4 points, curve is defined between points 1 and n-2.
    CatmullRom,
    /// B-Spline - smooth curve with local control.
    /// Does not pass through control points except endpoints.
    BSpline,
}

impl SplineType {
    /// Cycle to the next spline type.
    pub fn next(self) -> Self {
        match self {
            Self::CubicBezier => Self::CatmullRom,
            Self::CatmullRom => Self::BSpline,
            Self::BSpline => Self::CubicBezier,
        }
    }

    /// Get the display name for this spline type.
    pub fn name(&self) -> &'static str {
        match self {
            Self::CubicBezier => "Cubic Bézier",
            Self::CatmullRom => "Catmull-Rom",
            Self::BSpline => "B-Spline",
        }
    }

    /// Minimum number of control points required for this spline type.
    pub fn min_points(&self) -> usize {
        match self {
            Self::CubicBezier => 4,
            Self::CatmullRom => 4,
            Self::BSpline => 4,
        }
    }
}

/// Trait for evaluating spline curves.
pub trait SplineEvaluator {
    /// Evaluate the spline at parameter t (0.0 to 1.0 across entire spline).
    fn evaluate(&self, points: &[Vec3], t: f32, closed: bool) -> Option<Vec3>;

    /// Evaluate the tangent at parameter t.
    fn evaluate_tangent(&self, points: &[Vec3], t: f32, closed: bool) -> Option<Vec3>;

    /// Get the number of segments in the spline.
    fn segment_count(&self, points: &[Vec3], closed: bool) -> usize;
}

impl SplineEvaluator for SplineType {
    fn evaluate(&self, points: &[Vec3], t: f32, closed: bool) -> Option<Vec3> {
        match self {
            Self::CubicBezier => evaluate_cubic_bezier(points, t, closed),
            Self::CatmullRom => evaluate_catmull_rom(points, t, closed),
            Self::BSpline => evaluate_bspline(points, t, closed),
        }
    }

    fn evaluate_tangent(&self, points: &[Vec3], t: f32, closed: bool) -> Option<Vec3> {
        match self {
            Self::CubicBezier => evaluate_cubic_bezier_tangent(points, t, closed),
            Self::CatmullRom => evaluate_catmull_rom_tangent(points, t, closed),
            Self::BSpline => evaluate_bspline_tangent(points, t, closed),
        }
    }

    fn segment_count(&self, points: &[Vec3], closed: bool) -> usize {
        match self {
            Self::CubicBezier => {
                if points.len() < 4 {
                    0
                } else {
                    (points.len() - 1) / 3
                }
            }
            Self::CatmullRom | Self::BSpline => {
                if points.len() < 4 {
                    0
                } else if closed {
                    points.len()
                } else {
                    points.len() - 3
                }
            }
        }
    }
}

// Cubic Bézier implementation
fn evaluate_cubic_bezier(points: &[Vec3], t: f32, _closed: bool) -> Option<Vec3> {
    if points.len() < 4 {
        return None;
    }

    let num_segments = (points.len() - 1) / 3;
    if num_segments == 0 {
        return None;
    }

    let t_scaled = t * num_segments as f32;
    let segment = (t_scaled.floor() as usize).min(num_segments - 1);
    let local_t = t_scaled - segment as f32;

    let i = segment * 3;
    if i + 3 >= points.len() {
        return Some(points[points.len() - 1]);
    }

    let p0 = points[i];
    let p1 = points[i + 1];
    let p2 = points[i + 2];
    let p3 = points[i + 3];

    Some(cubic_bezier(p0, p1, p2, p3, local_t))
}

fn evaluate_cubic_bezier_tangent(points: &[Vec3], t: f32, _closed: bool) -> Option<Vec3> {
    if points.len() < 4 {
        return None;
    }

    let num_segments = (points.len() - 1) / 3;
    if num_segments == 0 {
        return None;
    }

    let t_scaled = t * num_segments as f32;
    let segment = (t_scaled.floor() as usize).min(num_segments - 1);
    let local_t = t_scaled - segment as f32;

    let i = segment * 3;
    if i + 3 >= points.len() {
        let i = (num_segments - 1) * 3;
        let p0 = points[i];
        let p1 = points[i + 1];
        let p2 = points[i + 2];
        let p3 = points[i + 3];
        return Some(cubic_bezier_derivative(p0, p1, p2, p3, 1.0));
    }

    let p0 = points[i];
    let p1 = points[i + 1];
    let p2 = points[i + 2];
    let p3 = points[i + 3];

    Some(cubic_bezier_derivative(p0, p1, p2, p3, local_t))
}

fn cubic_bezier(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;
    let mt = 1.0 - t;
    let mt2 = mt * mt;
    let mt3 = mt2 * mt;

    p0 * mt3 + p1 * 3.0 * mt2 * t + p2 * 3.0 * mt * t2 + p3 * t3
}

fn cubic_bezier_derivative(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let mt = 1.0 - t;
    let mt2 = mt * mt;

    (p1 - p0) * 3.0 * mt2 + (p2 - p1) * 6.0 * mt * t + (p3 - p2) * 3.0 * t2
}

// Catmull-Rom implementation
fn evaluate_catmull_rom(points: &[Vec3], t: f32, closed: bool) -> Option<Vec3> {
    if points.len() < 4 {
        return None;
    }

    let num_segments = if closed {
        points.len()
    } else {
        points.len() - 3
    };

    if num_segments == 0 {
        return None;
    }

    let t_scaled = t * num_segments as f32;
    let segment = (t_scaled.floor() as usize).min(num_segments - 1);
    let local_t = t_scaled - segment as f32;

    let (p0, p1, p2, p3) = if closed {
        let n = points.len();
        (
            points[(segment + n - 1) % n],
            points[segment % n],
            points[(segment + 1) % n],
            points[(segment + 2) % n],
        )
    } else {
        (
            points[segment],
            points[segment + 1],
            points[segment + 2],
            points[segment + 3],
        )
    };

    Some(catmull_rom(p0, p1, p2, p3, local_t))
}

fn evaluate_catmull_rom_tangent(points: &[Vec3], t: f32, closed: bool) -> Option<Vec3> {
    if points.len() < 4 {
        return None;
    }

    let num_segments = if closed {
        points.len()
    } else {
        points.len() - 3
    };

    if num_segments == 0 {
        return None;
    }

    let t_scaled = t * num_segments as f32;
    let segment = (t_scaled.floor() as usize).min(num_segments - 1);
    let local_t = t_scaled - segment as f32;

    let (p0, p1, p2, p3) = if closed {
        let n = points.len();
        (
            points[(segment + n - 1) % n],
            points[segment % n],
            points[(segment + 1) % n],
            points[(segment + 2) % n],
        )
    } else {
        (
            points[segment],
            points[segment + 1],
            points[segment + 2],
            points[segment + 3],
        )
    };

    Some(catmull_rom_derivative(p0, p1, p2, p3, local_t))
}

fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;

    0.5 * ((2.0 * p1)
        + (-p0 + p2) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * t3)
}

fn catmull_rom_derivative(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;

    0.5 * ((-p0 + p2)
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * 2.0 * t
        + (-p0 + 3.0 * p1 - 3.0 * p2 + p3) * 3.0 * t2)
}

// B-Spline implementation (uniform cubic)
fn evaluate_bspline(points: &[Vec3], t: f32, closed: bool) -> Option<Vec3> {
    if points.len() < 4 {
        return None;
    }

    let num_segments = if closed {
        points.len()
    } else {
        points.len() - 3
    };

    if num_segments == 0 {
        return None;
    }

    let t_scaled = t * num_segments as f32;
    let segment = (t_scaled.floor() as usize).min(num_segments - 1);
    let local_t = t_scaled - segment as f32;

    let (p0, p1, p2, p3) = if closed {
        let n = points.len();
        (
            points[segment % n],
            points[(segment + 1) % n],
            points[(segment + 2) % n],
            points[(segment + 3) % n],
        )
    } else {
        (
            points[segment],
            points[segment + 1],
            points[segment + 2],
            points[segment + 3],
        )
    };

    Some(bspline(p0, p1, p2, p3, local_t))
}

fn evaluate_bspline_tangent(points: &[Vec3], t: f32, closed: bool) -> Option<Vec3> {
    if points.len() < 4 {
        return None;
    }

    let num_segments = if closed {
        points.len()
    } else {
        points.len() - 3
    };

    if num_segments == 0 {
        return None;
    }

    let t_scaled = t * num_segments as f32;
    let segment = (t_scaled.floor() as usize).min(num_segments - 1);
    let local_t = t_scaled - segment as f32;

    let (p0, p1, p2, p3) = if closed {
        let n = points.len();
        (
            points[segment % n],
            points[(segment + 1) % n],
            points[(segment + 2) % n],
            points[(segment + 3) % n],
        )
    } else {
        (
            points[segment],
            points[segment + 1],
            points[segment + 2],
            points[segment + 3],
        )
    };

    Some(bspline_derivative(p0, p1, p2, p3, local_t))
}

fn bspline(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;
    let t3 = t2 * t;

    (1.0 / 6.0)
        * ((1.0 - 3.0 * t + 3.0 * t2 - t3) * p0
            + (4.0 - 6.0 * t2 + 3.0 * t3) * p1
            + (1.0 + 3.0 * t + 3.0 * t2 - 3.0 * t3) * p2
            + t3 * p3)
}

fn bspline_derivative(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let t2 = t * t;

    (1.0 / 6.0)
        * ((-3.0 + 6.0 * t - 3.0 * t2) * p0
            + (-12.0 * t + 9.0 * t2) * p1
            + (3.0 + 6.0 * t - 9.0 * t2) * p2
            + 3.0 * t2 * p3)
}
