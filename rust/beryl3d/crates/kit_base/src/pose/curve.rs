//! Cubic Bezier curves for interpolating poses.

use super::Curve;

impl Curve {
    /// CSS's `linear`: progress equals elapsed time.
    pub const LINEAR: Self = Self {
        x1: 0.0,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    };
    /// CSS's default `ease`, `cubic-bezier(0.25, 0.1, 0.25, 1)`.
    pub const EASE: Self = Self {
        x1: 0.25,
        y1: 0.1,
        x2: 0.25,
        y2: 1.0,
    };
    /// CSS's `ease-in`, `cubic-bezier(0.42, 0, 1, 1)`.
    pub const EASE_IN: Self = Self {
        x1: 0.42,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    };
    /// CSS's `ease-out`, `cubic-bezier(0, 0, 0.58, 1)`.
    pub const EASE_OUT: Self = Self {
        x1: 0.0,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    };
    /// CSS's `ease-in-out`, `cubic-bezier(0.42, 0, 0.58, 1)`.
    pub const EASE_IN_OUT: Self = Self {
        x1: 0.42,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    };

    /// The point of the curve at its Bezier parameter `s`, from 0 to 1: how much of the time
    /// has passed there and how far along the motion is. Unlike `ease` it searches for no
    /// parameter, so it costs a few multiplications, for code that needs moments spread over
    /// the motion rather than the motion at chosen times.
    #[must_use]
    pub fn point(self, s: f32) -> [f32; 2] {
        let bezier = |a: f32, b: f32| s * (3.0 * (1.0 - s) * ((1.0 - s) * a + s * b) + s * s);
        [bezier(self.x1, self.x2), bezier(self.y1, self.y2)]
    }

    /// How far along the motion is, from 0 to 1, when `x` of its time has passed. It finds
    /// the Bezier parameter whose x is `x` by Newton's method kept inside a bisection
    /// bracket, as `motion`'s `cubicBezier` does, and returns that parameter's y.
    #[must_use]
    pub fn ease(self, x: f32) -> f32 {
        let x = x.clamp(0.0, 1.0);
        if self == Self::LINEAR {
            return x;
        }
        let x = f64::from(x);
        let [x1, y1, x2, y2] = [self.x1, self.y1, self.x2, self.y2].map(f64::from);
        // The Bernstein form, exactly 0 at s = 0 and 1 at s = 1.
        let bezier =
            |s: f64, a: f64, b: f64| s * (3.0 * (1.0 - s) * ((1.0 - s) * a + s * b) + s * s);
        let slope = |s: f64, a: f64, b: f64| {
            3.0 * (1.0 - s) * (1.0 - s) * a
                + 6.0 * (1.0 - s) * s * (b - a)
                + 3.0 * s * s * (1.0 - b)
        };
        let (mut lower, mut upper, mut s) = (0.0, 1.0, x);
        for _ in 0..40 {
            if !(lower..=upper).contains(&s) {
                s = f64::midpoint(lower, upper);
            }
            let error = bezier(s, x1, x2) - x;
            if error * error < 1e-24 {
                break;
            }
            if error > 0.0 {
                upper = s;
            } else {
                lower = s;
            }
            let d = slope(s, x1, x2);
            s = if d.abs() < 1e-12 {
                f64::NAN
            } else {
                s - error / d
            };
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the result is between 0 and 1"
        )]
        let y = bezier(s.clamp(0.0, 1.0), y1, y2) as f32;
        y
    }
}
