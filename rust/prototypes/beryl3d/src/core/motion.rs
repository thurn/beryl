//! Poses, easing curves and segments: where an object is at a time is a pure function of the
//! segments it holds (docs/beryl-3d.md, "Determinism"), never a sum of steps.

use std::f32::consts::PI;

use glam::{Mat4, Quat, Vec3};

/// Where one object is and how it is turned. A card's face turning over is part of `rotation`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
    pub opacity: f32,
}

impl Pose {
    pub const HIDDEN: Pose = Pose {
        position: Vec3::ZERO,
        rotation: Quat::IDENTITY,
        scale: 1.0,
        opacity: 0.0,
    };

    pub fn at(position: Vec3, rotation: Quat) -> Pose {
        Pose {
            position,
            rotation,
            scale: 1.0,
            opacity: 1.0,
        }
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(Vec3::splat(self.scale), self.rotation, self.position)
    }

    pub fn with_opacity(self, opacity: f32) -> Pose {
        Pose { opacity, ..self }
    }

    /// Whether two poses would draw the same, to within a hundredth of a millimetre.
    pub fn same(&self, other: &Pose) -> bool {
        self.position.distance_squared(other.position) < 1e-4
            && self.rotation.dot(other.rotation).abs() > 0.999_999
            && (self.scale - other.scale).abs() < 1e-5
            && (self.opacity - other.opacity).abs() < 1e-3
    }
}

/// A cubic Bezier easing curve through (0, 0) and (1, 1), as CSS writes them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

impl Curve {
    pub const LINEAR: Curve = Curve {
        x1: 0.0,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    };
    pub const EASE_OUT: Curve = Curve {
        x1: 0.0,
        y1: 0.0,
        x2: 0.58,
        y2: 1.0,
    };
    pub const EASE_IN: Curve = Curve {
        x1: 0.42,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    };

    /// The curve's progress at time fraction `t`, solving x(s) = t by Newton's method.
    pub fn eval(&self, t: f32) -> f32 {
        if *self == Curve::LINEAR {
            return t;
        }
        let bezier = |a: f32, b: f32, s: f32| {
            let inv = 1.0 - s;
            3.0 * inv * inv * s * a + 3.0 * inv * s * s * b + s * s * s
        };
        let slope = |a: f32, b: f32, s: f32| {
            let inv = 1.0 - s;
            3.0 * inv * inv * a + 6.0 * inv * s * (b - a) + 3.0 * s * s * (1.0 - b)
        };
        let mut s = t;
        for _ in 0..8 {
            let error = bezier(self.x1, self.x2, s) - t;
            let d = slope(self.x1, self.x2, s);
            if error.abs() < 1e-5 || d.abs() < 1e-6 {
                break;
            }
            s = (s - error / d).clamp(0.0, 1.0);
        }
        bezier(self.y1, self.y2, s)
    }
}

/// How long a stretch of animation lasts and how it eases, in nanoseconds.
#[derive(Clone, Copy, Debug)]
pub struct Timing {
    pub delay: u64,
    pub duration: u64,
    pub stagger: u64,
    pub curve: Curve,
}

/// One stretch of animation between two poses.
#[derive(Clone, Copy, Debug)]
pub struct Segment {
    pub start: u64,
    pub duration: u64,
    pub curve: Curve,
    pub from: Pose,
    pub to: Pose,
    /// How far the path rises above the straight line at its middle, in millimetres.
    pub arc: f32,
}

impl Segment {
    pub fn new(start: u64, timing: Timing, from: Pose, to: Pose, arc: f32) -> Segment {
        Segment {
            start,
            duration: timing.duration,
            curve: timing.curve,
            from,
            to,
            arc,
        }
    }

    /// A change with no animation at `start`.
    pub fn instant(start: u64, to: Pose) -> Segment {
        Segment {
            start,
            duration: 0,
            curve: Curve::LINEAR,
            from: to,
            to,
            arc: 0.0,
        }
    }

    pub fn end(&self) -> u64 {
        self.start + self.duration
    }

    pub fn eval(&self, t: u64) -> Pose {
        if t >= self.end() || self.duration == 0 {
            return self.to;
        }
        let linear = t.saturating_sub(self.start) as f32 / self.duration as f32;
        let u = self.curve.eval(linear);
        let lift = self.arc * (PI * linear).sin();
        Pose {
            position: self.from.position.lerp(self.to.position, u) + Vec3::Z * lift,
            rotation: self.from.rotation.slerp(self.to.rotation, u),
            scale: self.from.scale + (self.to.scale - self.from.scale) * u,
            opacity: self.from.opacity + (self.to.opacity - self.from.opacity) * u,
        }
    }
}

/// The most segments an object holds at once. A script compiled ahead of time can give one
/// object several (drawn, then cast, then resolved).
const SEGMENTS: usize = 8;

/// An object's segments, sorted by start time, in a fixed array so that adding one never
/// allocates.
#[derive(Clone, Copy)]
pub struct Track {
    segments: [Segment; SEGMENTS],
    len: usize,
}

impl Default for Track {
    fn default() -> Self {
        Track {
            segments: [Segment::instant(0, Pose::HIDDEN); SEGMENTS],
            len: 0,
        }
    }
}

impl Track {
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The pose at `t`: the latest segment that has started, evaluated at `t`.
    pub fn pose_at(&self, t: u64) -> Pose {
        let live = &self.segments[..self.len];
        let Some(first) = live.first() else {
            return Pose::HIDDEN;
        };
        let mut current = first;
        if t < first.start {
            return first.from;
        }
        for segment in live {
            if segment.start <= t {
                current = segment;
            } else {
                break;
            }
        }
        current.eval(t)
    }

    /// Whether a segment is moving the object at `t`.
    pub fn moving(&self, t: u64) -> bool {
        self.segments[..self.len]
            .iter()
            .any(|s| s.start <= t && t < s.end() && !s.from.same(&s.to))
    }

    pub fn last_start(&self) -> u64 {
        self.segments[..self.len].last().map_or(0, |s| s.start)
    }

    /// The pose after every segment has ended.
    pub fn final_pose(&self) -> Pose {
        self.segments[..self.len]
            .last()
            .map_or(Pose::HIDDEN, |s| s.to)
    }

    /// Adds `segment` in start order. When the array is full, segments that ended before `now`
    /// are dropped first, keeping the latest of them, which still gives the pose until the next
    /// one starts.
    pub fn push(&mut self, segment: Segment, now: u64) {
        if self.len == SEGMENTS {
            self.compact(now);
        }
        if self.len == SEGMENTS {
            // Still full: the oldest segment is the one least likely to be shown again.
            self.segments.copy_within(1.., 0);
            self.len -= 1;
        }
        let mut at = self.len;
        while at > 0 && self.segments[at - 1].start > segment.start {
            at -= 1;
        }
        self.segments.copy_within(at..self.len, at + 1);
        self.segments[at] = segment;
        self.len += 1;
    }

    fn compact(&mut self, now: u64) {
        let ended = self.segments[..self.len]
            .iter()
            .take_while(|s| s.end() <= now)
            .count();
        if ended > 1 {
            let drop = ended - 1;
            self.segments.copy_within(drop..self.len, 0);
            self.len -= drop;
        }
    }
}
