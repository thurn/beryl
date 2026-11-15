//! Where an object is at a time, worked out from the segments it holds (docs/beryl-3d.md,
//! "Determinism": "Positions are functions of time"). Every function here is pure: the same
//! segments and the same time give the same pose, however the frames before it went.

use std::f32::consts::PI;

use beryl_protocol::script::Curve as Named;
use glam::Vec3;

use crate::pose::{Curve, Pose, Segment, Shape};
use crate::verbs::Interval;

impl Curve {
    /// CSS's `linear`: progress equals elapsed time.
    pub const LINEAR: Self = Self {
        x1: 0.0,
        y1: 0.0,
        x2: 1.0,
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

    /// The curve a step's timing names. These are the curves the web interface's `CURVES`
    /// (`ui/src/stage/timing.ts`) maps the same names to: `motion`'s `easeIn`, `easeOut` and
    /// `easeInOut`, which are CSS's.
    #[must_use]
    pub const fn named(curve: Named) -> Self {
        match curve {
            Named::Linear => Self::LINEAR,
            Named::EaseIn => Self::EASE_IN,
            Named::EaseOut => Self::EASE_OUT,
            Named::EaseInOut => Self::EASE_IN_OUT,
        }
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

impl Segment {
    /// When it ends, in nanoseconds since the client started.
    #[must_use]
    pub const fn end(&self) -> u64 {
        self.start.saturating_add(self.duration)
    }

    /// The pose it gives at `t`: its start pose before it begins, its end pose once it has
    /// ended, and in between each part of the pose eased along its curve. The path rises
    /// `arc` millimetres above the straight line (toward the camera, `z`) at the middle of
    /// its time, along half a sine wave.
    #[must_use]
    pub fn pose_at(&self, t: u64) -> Pose {
        if t <= self.start && self.duration > 0 {
            return self.from;
        }
        if t >= self.end() {
            return self.to;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a segment lasts seconds, far below f64's exact range"
        )]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the fraction is between 0 and 1"
        )]
        let elapsed = ((t - self.start) as f64 / self.duration as f64) as f32;
        let u = self.curve.ease(elapsed);
        let (from, to) = (&self.from, &self.to);
        let position = match self.shape {
            Shape::Line => {
                from.position.lerp(to.position, u) + Vec3::Z * self.arc * (PI * elapsed).sin()
            }
        };
        Pose {
            position,
            rotation: from.rotation.slerp(to.rotation, u),
            scale: from.scale + (to.scale - from.scale) * u,
            opacity: from.opacity + (to.opacity - from.opacity) * u,
            turn: from.turn + (to.turn - from.turn) * u,
        }
    }
}

impl Segment {
    /// Changes the start pose so that the pose at `t` is `pose`, keeping the start, duration,
    /// curve, end pose and arc. When the window changes shape during a flight, the segment under
    /// way takes the new end pose and this keeps the card where it is at that instant, so it
    /// does not jump (docs/beryl-3d.md, "Determinism"). A segment not under way at `t` is left
    /// as it is, and so is one too near its end for the start pose to matter.
    pub fn through(&mut self, t: u64, pose: Pose) {
        if t < self.start || t >= self.end() || self.duration == 0 {
            return;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a segment lasts seconds, far below f64's exact range"
        )]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the fraction is between 0 and 1"
        )]
        let elapsed = ((t - self.start) as f64 / self.duration as f64) as f32;
        let u = self.curve.ease(elapsed);
        let rest = 1.0 - u;
        if rest < 1e-4 {
            return;
        }
        let to = self.to;
        // Each part of the pose is `from + (to - from) * u`, so `from = (now - to * u) / rest`.
        let back = |now: f32, to: f32| (now - to * u) / rest;
        let lift = Vec3::Z * self.arc * (PI * elapsed).sin();
        // `slerp(from, to, u)` is `to * (to⁻¹ * from)^(1 - u)`, so `to⁻¹ * from` is
        // `(to⁻¹ * now)^(1 / rest)`.
        let (axis, angle) = (to.rotation.inverse() * pose.rotation).to_axis_angle();
        self.from = Pose {
            position: (pose.position - lift - to.position * u) / rest,
            rotation: to.rotation * glam::Quat::from_axis_angle(axis, angle / rest),
            scale: back(pose.scale, to.scale),
            opacity: back(pose.opacity, to.opacity),
            turn: back(pose.turn, to.turn),
        };
    }
}

/// An object's pose at `t`, from its `segments` sorted by start. The latest segment that has
/// begun by `t` decides it, so a segment that begins replaces any still under way. Before
/// the first begins the object stands at that segment's start pose, and with no segments at
/// all it stands at the default pose.
#[must_use]
pub fn pose_at(segments: &[Segment], t: u64) -> Pose {
    let begun = segments.partition_point(|segment| segment.start <= t);
    match (begun.checked_sub(1), segments.first()) {
        (Some(latest), _) => segments[latest].pose_at(t),
        (None, Some(first)) => first.from,
        (None, None) => Pose::default(),
    }
}

/// The most segments one object holds at once. A script laid out ahead of time can give one
/// card several that lie in the future: drawn, held up to be read, then cast and resolved, each
/// with the slides and holds round it.
pub const SEGMENTS: usize = 16;

/// A track already holds `SEGMENTS` segments and none of them can be dropped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Full;

/// One object's segments, sorted by start, in a fixed array so that adding one never
/// allocates.
#[derive(Clone, Copy, Debug)]
pub struct Track {
    segments: [Segment; SEGMENTS],
    len: usize,
}

impl Default for Track {
    fn default() -> Self {
        let rest = Segment::between(
            Pose::default(),
            Pose::default(),
            Interval { from: 0, to: 0 },
            Curve::LINEAR,
        );
        Self {
            segments: [rest; SEGMENTS],
            len: 0,
        }
    }
}

impl Track {
    /// The segments it holds, sorted by start.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        &self.segments[..self.len]
    }

    /// The segments it holds, to change their poses. Their starts must not change, so that
    /// they stay sorted.
    pub fn segments_mut(&mut self) -> &mut [Segment] {
        &mut self.segments[..self.len]
    }

    /// The object's pose at `t` (`pose_at`).
    #[must_use]
    pub fn pose_at(&self, t: u64) -> Pose {
        pose_at(self.segments(), t)
    }

    /// Whether the object is moving at `t`: a segment under way at `t` changes its pose. A
    /// segment that holds the object still does not count.
    #[must_use]
    pub fn moving(&self, t: u64) -> bool {
        self.segments().iter().any(|segment| {
            segment.start <= t
                && t < segment.end()
                && (segment.from != segment.to || segment.arc != 0.0)
        })
    }

    /// Adds `segment` after every segment that starts no later than it, so that of two that
    /// start together the one added last decides the pose. When the track is full it first
    /// drops the segments that ended by `now`, all but the latest of them: that one still
    /// gives the pose until the next begins, and no time from `now` on can reach the others,
    /// because a later segment has begun. A segment that has not ended is never dropped,
    /// since a script laid out ahead of time holds segments that lie in the future, so with
    /// none ended a full track refuses the new one.
    ///
    /// # Errors
    ///
    /// `Full` when the track holds `SEGMENTS` segments and at most one of them has ended.
    pub fn push(&mut self, segment: Segment, now: u64) -> Result<(), Full> {
        if self.len == SEGMENTS {
            self.drop_ended(now);
        }
        if self.len == SEGMENTS {
            return Err(Full);
        }
        let at = self
            .segments()
            .partition_point(|s| s.start <= segment.start);
        self.segments.copy_within(at..self.len, at + 1);
        self.segments[at] = segment;
        self.len += 1;
        Ok(())
    }

    /// Drops every segment that begins after `t`, and says whether there was one.
    pub fn cut_after(&mut self, t: u64) -> bool {
        let kept = self.segments().partition_point(|s| s.start <= t);
        let cut = kept < self.len;
        self.len = kept;
        cut
    }

    /// Drops every segment that ended by `now` except the latest of them.
    fn drop_ended(&mut self, now: u64) {
        let Some(kept) = self.segments().iter().rposition(|s| s.end() <= now) else {
            return;
        };
        let mut len = 0;
        for i in 0..self.len {
            if i == kept || self.segments[i].end() > now {
                self.segments[len] = self.segments[i];
                len += 1;
            }
        }
        self.len = len;
    }
}
