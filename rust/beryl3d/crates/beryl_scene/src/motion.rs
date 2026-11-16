//! Where an object is at a time, worked out from the segments it holds (docs/beryl-3d.md,
//! "Determinism": "Positions are functions of time"). Every function here is pure: the same
//! segments and the same time give the same pose, however the frames before it went.

use std::f32::consts::PI;

use arrayvec::ArrayVec;
use beryl_protocol::script::Curve as Named;
use glam::{Quat, Vec3};

use crate::data::space::verbs;
use crate::data::space::verbs::fly::thrown;
use crate::pose::{Curve, Pose, Segment, Shape};
use crate::verbs::Follows;

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

/// Where a travelling segment's path is at one moment, besides the straight way between its
/// two poses.
struct Along {
    /// How far along the straight way it is, from 0 to 1. Every part of the pose but the
    /// position's offset and a `Thrown` or `Held` path's scale is eased by it.
    u: f32,
    /// How far the position is off the straight way, in millimetres.
    offset: Vec3,
    /// The turn it leans by, applied after the eased rotation.
    lean: Quat,
    /// How much its scale is over the straight change from one scale to the other.
    grow: f32,
}

/// How high a thrown permanent is `elapsed` of its time through, as a share of its greatest
/// height: up quickly, level, then down evenly (the web interface's `liftAt`).
fn thrown_height(elapsed: f32) -> f32 {
    let at = elapsed.clamp(0.0, 1.0);
    if at < thrown::RISEN {
        1.0 - (1.0 - at / thrown::RISEN).powi(2)
    } else if at <= thrown::HELD {
        1.0
    } else {
        1.0 - (at - thrown::HELD) / (1.0 - thrown::HELD)
    }
}

/// The scale a thrown permanent comes down through from `aloft` to `rest`, `elapsed` of its
/// time through (the web interface's `down` in `lifted`).
fn thrown_down(aloft: f32, rest: f32, elapsed: f32) -> f32 {
    rest + (aloft - rest) * (1.0 - elapsed.powf(1.5))
}

impl Segment {
    /// When it ends, in nanoseconds since the client started.
    #[must_use]
    pub const fn end(&self) -> u64 {
        self.start.saturating_add(self.duration)
    }

    /// Whether it changes the pose, rather than holding the object still.
    #[must_use]
    pub fn moves(&self) -> bool {
        self.from != self.to || self.arc != 0.0
    }

    /// Whether it changes the pose in a way the eye can follow: as `moves`, unless it only
    /// changes how high the object lies above the table. A card of a row lies one layer above
    /// the card before it, so when a card leaves the row every card after it drops one layer at
    /// once, and a card resting undrawn off the row's end would otherwise count as moved by that
    /// and be drawn there for the rest of the script.
    #[must_use]
    pub fn shifts(&self) -> bool {
        let restacked = Pose {
            position: self.from.position.with_z(self.to.position.z),
            ..self.from
        };
        self.moves() && (self.arc != 0.0 || restacked != self.to)
    }

    /// The moments besides its start, middle and end, which every check already looks at, when
    /// something can go wrong: when a thrown permanent is first at its greatest height, and when
    /// a card that turns over at a moment of its own begins and ends turning.
    pub fn moments(&self) -> impl Iterator<Item = u64> + use<> {
        let (start, duration) = (self.start, self.duration);
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a segment lasts seconds, far below f64's exact range"
        )]
        let at =
            move |share: f32| start + (duration as f64 * f64::from(share.clamp(0.0, 1.0))) as u64;
        let peak = match self.shape {
            Shape::Thrown { .. } => Some(at(thrown::RISEN)),
            _ => None,
        };
        let turning = self.turning.map(|turning| turning.span().map(at));
        peak.into_iter().chain(turning.into_iter().flatten())
    }

    /// The fraction of its time that has passed at `t`, which must be under way.
    fn elapsed(&self, t: u64) -> f32 {
        #[expect(
            clippy::cast_precision_loss,
            reason = "a segment lasts seconds, far below f64's exact range"
        )]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "the fraction is between 0 and 1"
        )]
        let elapsed = ((t - self.start) as f64 / self.duration as f64) as f32;
        elapsed
    }

    /// Where a travelling path is `elapsed` of its time through: a line, bowed, held or thrown,
    /// each rising `arc` above the straight way at the middle of its time. The other shapes keep to
    /// the straight way, eased by the curve.
    fn along(&self, elapsed: f32) -> Along {
        let eased = self.curve.ease(elapsed);
        let rise = Vec3::Z * self.arc * (PI * elapsed).sin();
        match self.shape {
            Shape::Bowed(bow) | Shape::BowedNear { bow, .. } => {
                let arch = bow.arch(eased);
                Along {
                    u: eased,
                    offset: rise + bow.offset * arch,
                    lean: Quat::from_axis_angle(bow.axis, bow.lean * arch),
                    grow: 0.0,
                }
            }
            Shape::Held { bow, lean, .. } => {
                let arch = bow.arch(eased);
                let leaning = (PI * (elapsed / self.edge_on()).min(1.0)).sin();
                Along {
                    u: eased,
                    offset: rise + bow.offset * arch,
                    lean: Quat::from_axis_angle(bow.axis, bow.lean * arch + lean * leaning),
                    grow: 0.0,
                }
            }
            Shape::Thrown { rise: up, .. } => Along {
                u: 1.0 - (1.0 - eased).powf(thrown::LAUNCH),
                offset: rise + up * thrown_height(elapsed),
                lean: Quat::IDENTITY,
                grow: 0.0,
            },
            Shape::Swept { bulge, grow } => {
                let curved = 2.0 * eased * (1.0 - eased);
                Along {
                    u: eased,
                    offset: rise + bulge * curved,
                    lean: Quat::IDENTITY,
                    grow: grow * curved,
                }
            }
            _ => Along {
                u: eased,
                offset: rise,
                lean: Quat::IDENTITY,
                grow: 0.0,
            },
        }
    }

    /// A thrown permanent's scale `elapsed` of its time through: from its start pose's to the
    /// first scale of its way down within `thrown::SHRUNK` of its time, then down to its end
    /// pose's.
    fn thrown_scale(&self, aloft: f32, elapsed: f32) -> f32 {
        let (from, to) = (self.from.scale, self.to.scale);
        if elapsed < thrown::SHRUNK {
            let first = thrown_down(aloft, to, thrown::SHRUNK);
            from + (first - from) * (elapsed / thrown::SHRUNK)
        } else {
            thrown_down(aloft, to, elapsed)
        }
    }

    /// When, as a share of its time, it is edge-on as it turns over: its `turning`'s `at`, or
    /// its start where it does not turn.
    fn edge_on(&self) -> f32 {
        self.turning.map_or(0.0, |turning| turning.at)
    }

    /// How much of its change from its start pose's size on the screen to its end pose's a
    /// `Shape::Held` flight has made `elapsed` of its time through (the web interface's `grown`
    /// in `ui/src/stage/drawn.ts`): none until it is edge-on, then faster and slower again along a
    /// smoothstep until it arrives.
    fn held_grown(&self, elapsed: f32) -> f32 {
        let at = self.edge_on();
        let after = ((elapsed - at) / (1.0 - at)).clamp(0.0, 1.0);
        after * after * (3.0 - 2.0 * after)
    }

    /// How much of its turn the object has made `elapsed` of its time through: by its
    /// `turning` if it has one, else as far as the eased progress `u`.
    fn turned(&self, elapsed: f32, u: f32) -> f32 {
        self.turning.map_or(u, |turning| turning.turned(elapsed))
    }

    /// A travelling path's pose `elapsed` of its time through.
    fn travelled(&self, elapsed: f32) -> Pose {
        let Along {
            u,
            offset,
            lean,
            grow,
        } = self.along(elapsed);
        let pose = between(&self.from, &self.to, u);
        let scale = match self.shape {
            Shape::Thrown { aloft, .. } => self.thrown_scale(aloft, elapsed),
            Shape::Held { bow, eye, .. } => {
                let depth = |at: Vec3| eye - at.dot(bow.axis);
                let seen = |pose: &Pose| pose.scale / depth(pose.position);
                let (from, to) = (seen(&self.from), seen(&self.to));
                let position = pose.position + offset;
                (from + (to - from) * self.held_grown(elapsed)) * depth(position)
            }
            _ => pose.scale + grow,
        };
        let turned = self.turned(elapsed, u);
        let position = pose.position + offset;
        // A lifted card is drawn nearer the eye along its line of sight, and smaller by as
        // much, so that on screen it is where the straight way has it.
        let (position, scale) = match self.shape {
            Shape::Lifted(lift) | Shape::BowedNear { lift, .. } => {
                let nearer = lift.nearer(position, u);
                (lift.eye + (position - lift.eye) * nearer, scale * nearer)
            }
            _ => (position, scale),
        };
        Pose {
            position,
            rotation: lean * pose.rotation,
            scale,
            turn: self.from.turn + (self.to.turn - self.from.turn) * turned,
            ..pose
        }
    }

    /// The pose it gives at `t`: its start pose before it begins, its end pose once it has
    /// ended, and in between as its `shape` says. Along a line, bowed or thrown path each part of
    /// the pose is eased along its curve, the path leaves the straight way as its shape says, and
    /// it rises `arc` millimetres above the table (`z`) at the middle of its time, along half a
    /// sine wave.
    #[must_use]
    pub fn pose_at(&self, t: u64) -> Pose {
        if t <= self.start && self.duration > 0 {
            return self.from;
        }
        if t >= self.end() {
            return self.to;
        }
        let elapsed = self.elapsed(t);
        match self.shape {
            Shape::Line
            | Shape::Bowed(_)
            | Shape::BowedNear { .. }
            | Shape::Held { .. }
            | Shape::Thrown { .. }
            | Shape::Swept { .. }
            | Shape::Lifted(_) => self.travelled(elapsed),
            Shape::TurnedOver => turned_over(&self.from, &self.to, elapsed),
            Shape::Grown => {
                let u = self.curve.ease(elapsed);
                let pose = between(&self.from, &self.to, u);
                let size = verbs::showcase::SIZE;
                Pose {
                    scale: pose.scale * (size + (1.0 - size) * u),
                    opacity: pose.opacity * (u / verbs::showcase::SEEN).min(1.0),
                    ..pose
                }
            }
            Shape::Fading => {
                let u = self.curve.ease(elapsed);
                Pose {
                    opacity: self.from.opacity * (1.0 - u * u),
                    ..self.from
                }
            }
        }
    }
}

/// The pose `u` of the way from `from` to `to`, each part of it alike.
pub(crate) fn between(from: &Pose, to: &Pose, u: f32) -> Pose {
    Pose {
        position: from.position.lerp(to.position, u),
        rotation: from.rotation.slerp(to.rotation, u),
        scale: from.scale + (to.scale - from.scale) * u,
        opacity: from.opacity + (to.opacity - from.opacity) * u,
        turn: from.turn + (to.turn - from.turn) * u,
    }
}

/// A card turning over where it lies, `elapsed` of the way through (`Shape::TurnedOver`), as
/// the web interface's `turnedOver` (`ui/src/stage/tracks.ts`) has it: it rises and grows
/// quickly and then slowing into its largest by `grown`, stays up, and from `settle` comes back
/// down, slowing onto the table. It turns over while it is up, evenly from `grown` to `settle`,
/// so that it is edge-on, and shows its new side, at the middle of its time.
fn turned_over(from: &Pose, to: &Pose, elapsed: f32) -> Pose {
    let (grown, settle) = (verbs::flip::GROWN, verbs::flip::SETTLE);
    let up = if elapsed < grown {
        Curve::EASE_OUT.ease(elapsed / grown)
    } else if elapsed < settle {
        1.0
    } else {
        1.0 - Curve::EASE_IN_OUT.ease((elapsed - settle) / (1.0 - settle))
    };
    let pose = between(from, to, elapsed);
    let turned = ((elapsed - grown) / (settle - grown)).clamp(0.0, 1.0);
    Pose {
        position: pose.position + Vec3::Z * verbs::flip::RISE * up,
        scale: pose.scale * (1.0 + (verbs::flip::SCALE - 1.0) * up),
        turn: from.turn + (to.turn - from.turn) * turned,
        ..pose
    }
}

impl Segment {
    /// Changes the start pose so that the pose at `t` is `pose`, keeping the start, duration,
    /// curve, end pose and path. When the window changes shape during a flight, the segment under
    /// way takes the new end pose and this keeps the card where it is at that instant, so it
    /// does not jump (docs/beryl-3d.md, "Determinism"). A segment not under way at `t` is left
    /// as it is, and so is one too near its end for the start pose to matter. A thrown
    /// permanent's scale after its first frames does not depend on its start pose, so there its
    /// `aloft` scale changes instead.
    pub fn through(&mut self, t: u64, pose: Pose) {
        if t < self.start || t >= self.end() || self.duration == 0 {
            return;
        }
        let elapsed = self.elapsed(t);
        if self.shape == Shape::Fading {
            // It stands where it was all the way, only fading, so it starts where it is now.
            let u = self.curve.ease(elapsed);
            let left = 1.0 - u * u;
            if left >= 1e-4 {
                self.from = Pose {
                    opacity: pose.opacity / left,
                    ..pose
                };
            }
            return;
        }
        let Along {
            u,
            offset,
            lean,
            grow,
        } = self.along(elapsed);
        let rest = 1.0 - u;
        if rest < 1e-4 {
            return;
        }
        let to = self.to;
        // Each part of the pose is `from + (to - from) * u`, so `from = (now - to * u) / rest`.
        let back = |now: f32, to: f32| (now - to * u) / rest;
        // `slerp(from, to, u)` is `to * (to⁻¹ * from)^(1 - u)`, so `to⁻¹ * from` is
        // `(to⁻¹ * now)^(1 / rest)`, where `now` is the pose's rotation before its lean.
        let unleaned = lean.inverse() * pose.rotation;
        let (axis, angle) = (to.rotation.inverse() * unleaned).to_axis_angle();
        let grown = self.held_grown(elapsed);
        let scale = match &mut self.shape {
            Shape::Thrown { aloft, .. } if elapsed < thrown::SHRUNK => {
                let share = elapsed / thrown::SHRUNK;
                let first = thrown_down(*aloft, to.scale, thrown::SHRUNK);
                (pose.scale - first * share) / (1.0 - share)
            }
            Shape::Thrown { aloft, .. } => {
                let left = 1.0 - elapsed.powf(1.5);
                if left > 1e-4 {
                    *aloft = to.scale + (pose.scale - to.scale) / left;
                }
                self.from.scale
            }
            // Set below, once the start's position is known.
            Shape::Held { .. } => self.from.scale,
            _ => back(pose.scale - grow, to.scale),
        };
        // The turn goes by its own share where the segment has a `turning`; once that share is
        // whole the start's turn no longer matters.
        let turned = self.turned(elapsed, u);
        let turn = if turned < 1.0 - 1e-4 {
            (pose.turn - to.turn * turned) / (1.0 - turned)
        } else {
            self.from.turn
        };
        let position = (pose.position - offset - to.position * u) / rest;
        // A held flight's size on the screen goes by its own share; once that share is whole
        // the start's no longer matters.
        let scale = match self.shape {
            Shape::Held { bow, eye, .. } if grown < 1.0 - 1e-4 => {
                let depth = |at: Vec3| eye - at.dot(bow.axis);
                let (now, to) = (
                    pose.scale / depth(pose.position),
                    to.scale / depth(to.position),
                );
                (now - to * grown) / (1.0 - grown) * depth(position)
            }
            _ => scale,
        };
        self.from = Pose {
            position,
            rotation: to.rotation * Quat::from_axis_angle(axis, angle / rest),
            scale,
            opacity: back(pose.opacity, to.opacity),
            turn,
        };
    }
}

/// An object's pose at `t`, from its `segments` sorted by start. The latest segment that has
/// begun by `t` decides it, so a segment that begins replaces any still under way. Before
/// the first begins the object stands at that segment's start pose, and with no segments at
/// all it stands at the default pose.
#[must_use]
pub fn pose_at(segments: &[Segment], t: u64) -> Pose {
    if let Some(segment) = segments.iter().rev().find(|segment| segment.start <= t) {
        segment.pose_at(t)
    } else {
        segments
            .first()
            .map_or_else(Pose::default, |first| first.from)
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
#[derive(Clone, Debug, Default)]
pub struct Track {
    segments: ArrayVec<Segment, SEGMENTS>,
}

impl Track {
    /// The segments it holds, sorted by start.
    #[must_use]
    pub fn segments(&self) -> &[Segment] {
        self.segments.as_slice()
    }

    /// The segments it holds, to change their poses. Their starts must not change, so that
    /// they stay sorted.
    pub fn segments_mut(&mut self) -> &mut [Segment] {
        self.segments.as_mut_slice()
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
        (self.segments().iter())
            .any(|segment| segment.start <= t && t < segment.end() && segment.moves())
    }

    /// Whether a segment that changes the object's pose has begun by `t`. Until one has, the
    /// object holds the pose it started the track in, or one only a layer higher or lower
    /// (`Segment::shifts`).
    #[must_use]
    pub fn begun(&self, t: u64) -> bool {
        (self.segments().iter()).any(|segment| segment.start <= t && segment.shifts())
    }

    /// Whether every segment that changes the object's pose has ended by `t`. From then on the
    /// object holds the pose it ends the track in.
    #[must_use]
    pub fn settled(&self, t: u64) -> bool {
        (self.segments().iter()).all(|segment| segment.end() <= t || !segment.shifts())
    }

    /// Whether a step's own act moves the object at `t`: a segment its verb made, or that
    /// brought it to the table or took it away, is under way and moves it more than a layer up
    /// or down (`Segment::shifts`). A segment the object slides along because a step changed
    /// where it rests does not count.
    #[must_use]
    pub fn carried(&self, t: u64) -> bool {
        self.segments().iter().any(|segment| {
            segment.start <= t
                && t < segment.end()
                && segment.shifts()
                && segment
                    .step
                    .is_some_and(|step| step.follows == Follows::Start)
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
        if self.segments.is_full() {
            self.drop_ended(now);
            if self.segments.is_full() {
                return Err(Full);
            }
        }
        if self
            .segments
            .last()
            .is_none_or(|last| last.start <= segment.start)
        {
            self.segments.push(segment);
        } else {
            let at = self.segments.partition_point(|s| s.start <= segment.start);
            self.segments.insert(at, segment);
        }
        Ok(())
    }

    /// Appends to a track already cut to the start of `segment`.
    pub(crate) fn append(&mut self, segment: Segment) {
        debug_assert!(
            self.segments()
                .last()
                .is_none_or(|last| last.start <= segment.start)
        );
        if self.segments.is_full() {
            self.drop_ended(segment.start);
            if self.segments.is_full() {
                self.segments.clear();
            }
        }
        self.segments.push(segment);
    }

    /// Moves each segment later by what `later` gives for it, keeping them sorted by start and,
    /// of two that start together, in the order they were added. Allocates nothing.
    pub fn shift(&mut self, later: impl Fn(&Segment) -> u64) {
        for segment in self.segments_mut() {
            segment.start += later(segment);
        }
        // An insertion sort, which is stable and allocates nothing; the segments are almost
        // always in order already.
        for i in 1..self.segments.len() {
            let mut j = i;
            while j > 0 && self.segments[j - 1].start > self.segments[j].start {
                self.segments.swap(j - 1, j);
                j -= 1;
            }
        }
    }

    /// Drops every segment that begins after `t`, and says whether there was one.
    pub fn cut_after(&mut self, t: u64) -> bool {
        if self.segments().last().is_none_or(|last| last.start <= t) {
            return false;
        }
        self.segments
            .truncate(self.segments.partition_point(|s| s.start <= t));
        true
    }

    /// Drops every segment that ended by `now` except the latest of them.
    fn drop_ended(&mut self, now: u64) {
        let Some(kept) = self.segments().iter().rposition(|s| s.end() <= now) else {
            return;
        };
        let mut index = 0;
        self.segments.retain(|s| {
            let retain = index == kept || s.end() > now;
            index += 1;
            retain
        });
    }
}
