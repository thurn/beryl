//! Where an object is, and the stretches of animation that move it (docs/beryl-3d.md,
//! "Determinism"). An object's pose at a time is computed from the segments it holds, never by
//! adding a step to the pose of the previous frame, so a late frame shows a later pose and any
//! moment can be drawn directly.

use std::f32::consts::PI;

use glam::{Affine3A, Mat3, Quat, Vec3};

mod curve;
mod segment;

pub use segment::between;

/// When one step plays on the script's clock, in nanoseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Interval {
    pub from: u64,
    pub to: u64,
}

impl Interval {
    #[must_use]
    pub const fn duration(self) -> u64 {
        self.to.saturating_sub(self.from)
    }
}

/// The step of a script that made something with a time, by its index in the script, and
/// which of the step's times it keeps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepTime {
    pub number: usize,
    pub follows: Follows,
}

/// Which of a step's times something it made keeps to. A step's end is always its start plus
/// its length, which does not change when its script is laid out again, so what keeps to its
/// end moves with its start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Follows {
    /// When the step begins, or ends.
    Start,
    /// The time the step leaves for what makes room or closes up round it (`room_of`), which
    /// begins as its act is applied when that goes ahead of the step (`Laid::opens`).
    Room,
}

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
    /// How far the card is sheared across its own plane, as the web's `skewX` leans a card on
    /// the table toward its vanishing point: the card's bottom edge lies `skew` times the card's
    /// height to the right of its top edge, measured in the card's frame after `rotation`.
    pub skew: f32,
    /// How far the object's far side moves across the table (along `x`) for each millimetre
    /// along it (along `y`) from its centre, after it is rotated and skewed: a shear in the
    /// table's plane rather than the card's, so that a tapped card leans as an untapped one
    /// does. It is nought except on a table row fitted to the web interface, whose cards
    /// lean toward the web's vanishing point as the web's skewed cards do (`web_fit::lean`).
    pub shear: f32,
}

impl Default for Pose {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: 1.0,
            opacity: 1.0,
            turn: 0.0,
            skew: 0.0,
            shear: 0.0,
        }
    }
}

impl Pose {
    /// The transform from a card mesh's space into the table's: scaled, turned over by `turn`
    /// top over bottom (about the card's short axis, as a card tossed across a table turns),
    /// rotated, skewed, then moved to `position`, and sheared in the table's plane by `shear`.
    #[must_use]
    pub fn transform(&self) -> Affine3A {
        self.sheared(self.placed(self.turn))
    }

    /// The transform of a quad `size` large, laid in the card's plane with its middle at
    /// `position` and turned and sheared as the card is, for what is drawn on or round a card
    /// (a mark's outline, an effect's burst) and has to follow its edges.
    #[must_use]
    pub fn frame(&self, size: Vec3, position: Vec3) -> Affine3A {
        if self.skew == 0.0 {
            return self.sheared(Affine3A::from_scale_rotation_translation(
                size,
                self.rotation,
                position,
            ));
        }
        self.sheared(
            Affine3A::from_rotation_translation(self.rotation, position)
                * self.shear()
                * Affine3A::from_scale(size),
        )
    }

    /// `transform`, which lays something on the object, sheared in the table's plane by
    /// `shear` about the object's centre, so that it lies on the object as drawn.
    #[must_use]
    pub fn sheared(&self, transform: Affine3A) -> Affine3A {
        if self.shear == 0.0 {
            return transform;
        }
        // x' = x + shear * y, about the object's centre.
        let shear = Mat3::from_cols(Vec3::X, Vec3::new(self.shear, 1.0, 0.0), Vec3::Z);
        let from = Vec3::from(transform.translation) - self.position;
        Affine3A::from_mat3_translation(
            shear * Mat3::from(transform.matrix3),
            self.position + shear * from,
        )
    }

    /// The shear of a skewed card in its own frame: a point's x moves left by `skew` times its
    /// y, so that the card's foot lies right of its top.
    fn shear(&self) -> Affine3A {
        Affine3A::from_mat3(Mat3::from_cols(
            Vec3::X,
            Vec3::new(-self.skew, 1.0, 0.0),
            Vec3::Z,
        ))
    }

    /// How the side of the card that faces up is turned: as `transform` turns the card while
    /// it turns over, edge-on half way, but always the side that faces up, so that what is
    /// drawn on the card (its plaque, its marks) turns with it and lies flat on whichever side
    /// it shows at rest, as the web interface's children of a card turn with it.
    #[must_use]
    pub fn upward(&self) -> Affine3A {
        self.sheared(self.placed(self.turn - self.turn.round()))
    }

    /// The transform of a card turned over by `turn`. A skewed card is sheared after it turns
    /// and before `rotation` lays it down, so that the shear leans the same way on screen
    /// whichever side shows.
    fn placed(&self, turn: f32) -> Affine3A {
        if self.skew == 0.0 {
            return Affine3A::from_scale_rotation_translation(
                Vec3::splat(self.scale),
                self.rotation_for(turn),
                self.position,
            );
        }
        let flipped = Quat::from_rotation_x(PI * turn);
        Affine3A::from_rotation_translation(self.rotation, self.position)
            * self.shear()
            * Affine3A::from_scale_rotation_translation(
                Vec3::splat(self.scale),
                flipped,
                Vec3::ZERO,
            )
    }

    fn rotation_for(&self, turn: f32) -> Quat {
        if turn == 0.0 {
            self.rotation
        } else {
            self.rotation * Quat::from_rotation_x(PI * turn)
        }
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
    TurnedOver(crate::data::verbs::Flip),
    /// A face coming to be seen where it rests (`showcase`): it grows from `verbs.showcase.size`
    /// of its size and is opaque once `verbs.showcase.seen` of its motion is done.
    Grown(crate::data::verbs::Showcase),
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
    /// size it had on the pile. `eye` is how far along `bow.axis` the camera's eye is, so
    /// that a position `p` is `eye - p · bow.axis` from it.
    Held { bow: Bow, lean: f32, eye: f32 },
    /// A permanent's flight off the stack onto its place, as the web interface's `lifted`
    /// (`ui/src/stage/lift.ts`) has it: it rises by `rise` (millimetres, up the screen; `fly`
    /// caps it so that the card never rises past the top of the window) at once,
    /// crosses most of the way at that height and drops onto its place. Its scale goes from its
    /// start pose's to `aloft` within the first frames and from there to its end pose's as it
    /// comes down. Its progress along the way is launched: it is half way across after about a
    /// quarter of its time. The fractions of its time are `verbs.fly.thrown` in
    /// `data/beryl3d/verbs.json`.
    Thrown {
        rise: Vec3,
        aloft: f32,
        tuning: crate::data::verbs::Thrown,
    },
    /// A curve that passes by a place without stopping there, as the web interface's `swept`
    /// (`ui/src/stage/drawn.ts`) has it: a curve of the second degree from the start pose to the
    /// end pose, along the eased progress `u`, whose position is `2 u (1 - u) bulge` off the
    /// straight way and whose scale is `2 u (1 - u) grow` over the straight change. `fly` makes
    /// both from the place passed by, so that the curve goes as far as it and no farther.
    Swept { bulge: Vec3, grow: f32 },
    /// A straight line along which the opacity follows a fade of its own rather than the
    /// segment's curve: a card fading in where it rests that its hand regrouped round, on its
    /// short way to its new place and for the rest of its fade there (`play::step::fade`).
    Regrouping(Fade),
}

/// A fade that a `Shape::Regrouping` segment's opacity follows. It began `begun` nanoseconds
/// before the segment does and lasts `duration` nanoseconds, from `from` to `to` along `curve`.
/// Its times count from the segment's start, so that a script laid out again later moves it
/// with the segment.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    pub begun: u64,
    pub duration: u64,
    pub curve: Curve,
    pub from: f32,
    pub to: f32,
}

impl Fade {
    /// The opacity `since` nanoseconds after the segment that follows it began.
    #[must_use]
    pub fn opacity(&self, since: u64) -> f32 {
        if self.duration == 0 {
            return self.to;
        }
        #[expect(
            clippy::cast_precision_loss,
            reason = "a fade lasts seconds, far below f64's exact range"
        )]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the fraction is between 0 and 1"
        )]
        let elapsed = ((self.begun + since) as f64 / self.duration as f64).clamp(0.0, 1.0) as f32;
        self.from + (self.to - self.from) * self.curve.ease(elapsed)
    }
}

pub use crate::data::verbs::Turn as Turning;

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
