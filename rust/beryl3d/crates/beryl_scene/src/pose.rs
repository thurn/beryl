//! Where an object is, and the stretches of animation that move it (docs/beryl-3d.md,
//! "Determinism"). An object's pose at a time is computed from the segments it holds, never by
//! adding a step to the pose of the previous frame, so a late frame shows a later pose and any
//! moment can be drawn directly.

use std::f32::consts::PI;

use glam::{Affine3A, Quat, Vec3};

use crate::verbs::{Interval, StepTime};

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
    /// top over bottom (about the card's short axis, as a card tossed across a table turns),
    /// rotated, then moved to `position`.
    #[must_use]
    pub fn transform(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(
            Vec3::splat(self.scale),
            self.rotation * Quat::from_rotation_x(PI * self.turn),
            self.position,
        )
    }

    /// How the side of the card that faces up is turned: as `transform` turns the card while
    /// it turns over, edge-on half way, but always the side that faces up, so that what is
    /// drawn on the card (its plaque, its marks) turns with it and lies flat on whichever side
    /// it shows at rest, as the web interface's children of a card turn with it.
    #[must_use]
    pub fn upward(&self) -> Affine3A {
        Affine3A::from_scale_rotation_translation(
            Vec3::splat(self.scale),
            self.rotation * Quat::from_rotation_x(PI * (self.turn - self.turn.round())),
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
/// line with a rise adds a variant here, and `Segment::pose_at` matches on it. Every shape that
/// travels also rises `Segment::arc` millimetres above the table (along `z`) at the middle of its
/// time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Shape {
    /// A straight line.
    #[default]
    Line,
    /// The object stays at `from` and fades as it goes, its opacity `from.opacity` times
    /// 1 - (eased progress)², as the web interface's `dissolve` (`ui/src/stage/tracks.ts`);
    /// once the segment has ended it is at `to`, with no path between the two.
    Fading,
    /// A card turning over where it lies (`flip`): it rises `verbs.flip.rise` millimetres and
    /// grows to `verbs.flip.scale` times its size by `verbs.flip.grown` of its time, turns over
    /// while it is up, crossing edge-on at the middle of its time, and from `verbs.flip.settle`
    /// of its time settles back to where it rests. Its time is shared out evenly: the step's
    /// curve does not bend it.
    TurnedOver,
    /// A face coming to be seen where it rests (`showcase`): it grows from `verbs.showcase.size`
    /// of its size and is opaque once `verbs.showcase.seen` of its motion is done.
    Grown,
    /// A line that leaves the straight way between its ends sideways on the screen and leans as
    /// it does, as the web interface's `bowed` (`ui/src/stage/bows.ts`) has it.
    Bowed(Bow),
    /// The flight in of a card the viewer draws alone and holds up to be read, when it turns
    /// over on the way, as the web interface's `heldUp` (`ui/src/stage/drawn.ts`) has it: bowed
    /// as `Bowed` is, and also leaning `lean` radians counterclockwise about the bow's axis by
    /// `sin(π min(1, elapsed / at))`, where `at` is when its `Segment::turning` has it edge-on,
    /// so that it is upright again by then. Its size on the screen, its scale over its distance
    /// from the eye, stays at its start pose's until it is edge-on, then changes to its end
    /// pose's along a smoothstep over the rest of its time, so that it turns over at about the
    /// size it had on the library. `eye` is how far along `bow.axis` the camera's eye is, so
    /// that a position `p` is `eye - p · bow.axis` from it.
    Held { bow: Bow, lean: f32, eye: f32 },
    /// A permanent's flight off the stack onto its place, as the web interface's `lifted`
    /// (`ui/src/stage/lift.ts`) has it: it rises by `rise` (millimetres, up the screen; `fly`
    /// caps it so that the card never rises past the top of the window) at once,
    /// crosses most of the way at that height and drops onto its place. Its scale goes from its
    /// start pose's to `aloft` within the first frames and from there to its end pose's as it
    /// comes down. Its progress along the way is launched: it is half way across after about a
    /// quarter of its time. The fractions of its time are `verbs.fly.thrown` in
    /// `data/beryl3d/space.json`.
    Thrown { rise: Vec3, aloft: f32 },
    /// A curve that passes by a place without stopping there, as the web interface's `swept`
    /// (`ui/src/stage/drawn.ts`) has it: a curve of the second degree from the start pose to the
    /// end pose, along the eased progress `u`, whose position is `2 u (1 - u) bulge` off the
    /// straight way and whose scale is `2 u (1 - u) grow` over the straight change. `fly` makes
    /// both from the place passed by, so that the curve goes as far as it and no farther.
    Swept { bulge: Vec3, grow: f32 },
    /// A straight line seen from the eye, with the card drawn nearer the eye than it lies: the
    /// viewer's card played from their hand onto the table, which leaves the hand in front of the
    /// cards beside it, as the web interface draws a flying card above every other.
    Lifted(Lift),
}

/// How a `Shape::Lifted` path holds its card near the eye. At each moment the card is moved
/// toward `eye` along the line from the eye through it and shrunk by the same factor, so that
/// it looks exactly as it would on the straight way and only its depth changes. Until `hold`
/// of its eased progress the card is no farther from the eye, along `forward`, than `near`
/// millimetres; from there it eases back to where it lies, which it reaches as it lands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lift {
    pub eye: Vec3,
    /// The unit direction the camera looks in.
    pub forward: Vec3,
    pub near: f32,
    pub hold: f32,
}

impl Lift {
    /// The factor by which a card at `position`, `u` of the eased way through, is drawn nearer
    /// the eye: 1 where it lies, less where it is held nearer.
    #[must_use]
    pub fn nearer(&self, position: Vec3, u: f32) -> f32 {
        let depth = (position - self.eye).dot(self.forward);
        let held = if depth > self.near {
            self.near / depth
        } else {
            1.0
        };
        let back = ((u - self.hold) / (1.0 - self.hold)).clamp(0.0, 1.0);
        let back = back * back * (3.0 - 2.0 * back);
        held + (1.0 - held) * back
    }
}

/// When a card turns over on its way, as shares of its segment's time (the web interface's
/// `Turning` in `ui/src/stage/tracks.ts`): its turn stays at its start value until `at - each`,
/// is half way at `at`, where the card is edge-on, and is at its end value from `at + each`,
/// turning evenly in between. Its time is not eased by the segment's curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Turning {
    pub at: f32,
    pub each: f32,
}

impl Turning {
    /// How much of its turn it has made `elapsed` of its time through, from 0 to 1.
    #[must_use]
    pub fn turned(self, elapsed: f32) -> f32 {
        f32::midpoint((elapsed - self.at) / self.each, 1.0).clamp(0.0, 1.0)
    }

    /// When it begins and ends turning, as shares of its time.
    #[must_use]
    pub fn span(self) -> [f32; 2] {
        [self.at - self.each, self.at + self.each]
    }
}

/// How far a `Shape::Bowed` path leaves its straight way, made for the two ends and the camera
/// it was given.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bow {
    /// How far off the straight way it is where it is farthest, in millimetres, in the plane
    /// facing the camera.
    pub offset: Vec3,
    /// `ln 0.5 / ln peak`, where `peak` is how far along the eased progress it is farthest off:
    /// the offset at progress `u` is `sin(π u^sharpness)` of `offset`.
    pub sharpness: f32,
    /// How far it turns about `axis` where it is farthest off, in radians, counterclockwise as
    /// the camera sees it. It is upright again as it lands.
    pub lean: f32,
    /// The unit direction toward the camera.
    pub axis: Vec3,
}

impl Bow {
    /// How much of its offset and lean it has at the eased progress `u`: 0 at either end and 1
    /// where it is farthest off.
    #[must_use]
    pub fn arch(&self, u: f32) -> f32 {
        (PI * u.max(0.0).powf(self.sharpness)).sin()
    }
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
    /// When its object turns over, if it does so at a moment of its own rather than evenly
    /// along the eased progress.
    pub turning: Option<Turning>,
    /// The step of its script that made it and which of that step's times it keeps to, so that
    /// a script laid out again as it starts can move it with that time; `None` for one no step
    /// made, which stays where it is.
    pub step: Option<StepTime>,
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
            turning: None,
            step: None,
        }
    }
}
