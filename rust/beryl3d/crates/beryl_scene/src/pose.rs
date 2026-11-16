//! Where an object is, and the stretches of animation that move it (docs/beryl-3d.md,
//! "Determinism"). An object's pose at a time is computed from the segments it holds, never by
//! adding a step to the pose of the previous frame, so a late frame shows a later pose and any
//! moment can be drawn directly.

use std::f32::consts::PI;

use glam::{Affine3A, Quat, Vec3};

use crate::verbs::Interval;

/// Where one object is and how it is turned, in the table's space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    /// The object's centre, in millimetres.
    pub position: Vec3,
    pub rotation: Quat,
    /// 1 at the object's natural size.
    pub scale: f32,
    /// From 0 (invisible) to 1 (opaque).
    pub opacity: f32,
    /// How far the face has turned over, from 0 (face up) to 1 (face down).
    pub turn: f32,
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: 1.0,
            opacity: 1.0,
            turn: 0.0,
        }
    }
}

impl Pose {
    /// The transform from a card mesh's space into the table's: scaled, turned over by `turn`
    /// about the card's long axis, rotated, then moved to `position`.
    #[must_use]
    pub fn transform(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(
            Vec3::splat(self.scale),
            self.rotation * Quat::from_rotation_y(PI * self.turn),
            self.position,
        )
    }
}

/// A cubic Bezier easing from (0, 0) to (1, 1), as CSS writes it: the control points
/// `(x1, y1)` and `(x2, y2)`, taken from the verb's data.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Curve {
    pub x1: f32,
    pub y1: f32,
    pub x2: f32,
    pub y2: f32,
}

/// The path a segment takes between its two poses. A verb whose motion is not a straight
/// line with a rise adds a variant here, and `Segment::pose_at` matches on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Shape {
    /// A straight line, rising `Segment::arc` millimetres above it at the middle of its time.
    #[default]
    Line,
    /// The object stays at `from` and fades as it goes, its opacity `from.opacity` times
    /// 1 - (eased progress)², as the web interface's `dissolve` (`ui/src/stage/tracks.ts`);
    /// once the segment has ended it is at `to`, with no path between the two.
    Fading,
}

/// One stretch of animation between two poses. Code outside this file makes one with
/// `Segment::between` and changes the fields it needs, as `Segment { arc, ..Segment::between(..) }`,
/// so that a field added later breaks none of it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    /// When it begins, in nanoseconds since the client started.
    pub start: u64,
    /// How long it lasts, in nanoseconds.
    pub duration: u64,
    pub curve: Curve,
    pub from: Pose,
    pub to: Pose,
    /// How far the path rises above the straight line between the two positions, in
    /// millimetres at its middle.
    pub arc: f32,
    pub shape: Shape,
}

impl Segment {
    /// A straight segment from `from` to `to` over `interval` along `curve`, with no rise.
    #[must_use]
    pub const fn between(from: Pose, to: Pose, interval: Interval, curve: Curve) -> Self {
        Self {
            start: interval.from,
            duration: interval.duration(),
            curve,
            from,
            to,
            arc: 0.0,
            shape: Shape::Line,
        }
    }
}
