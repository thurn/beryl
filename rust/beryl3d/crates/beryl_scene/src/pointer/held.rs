//! The card the pointer holds: where it is drawn, the lean of its spring, and its fall back to
//! where it rests once let go (docs/beryl-3d.md, "Motion the pointer drives").
//!
//! The values are those of the web interface's `Heft` (`ui/src/stage/heft.ts`), in
//! `data/beryl3d/space.json` under `pointer`. Lengths there are stage pixels and here logical
//! points.

use std::f32::consts::TAU;

use glam::{Quat, Vec2};

use crate::data::space::pointer as data;
use crate::motion::between;
use crate::pose::{Curve, Pose};
use crate::table::Camera;

/// Nanoseconds in a millisecond and in a second.
const MILLISECOND: f32 = 1_000_000.0;
const SECOND: f32 = 1_000_000_000.0;

/// A press on a card, from the button going down until it comes up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Grip {
    /// The slot of the card pressed.
    pub slot: usize,
    /// Where the press began and where the pointer is now, in logical points.
    pub start: Vec2,
    pub at: Vec2,
    /// Which drags pick the card up.
    pub hold: Hold,
    /// Whether the press has travelled `DRAG_THRESHOLD_POINTS` from where it began. A press
    /// that has is a drag, which is never a click on its card, whether or not it picked the
    /// card up: a drag across a battlefield row scrolls the row (`rows`).
    pub moved: bool,
    /// Whether the press has picked the card up: its `hold` let it, and it has `moved`.
    pub held: bool,
    /// What made the press.
    pub by: By,
}

/// Which drags of a press pick its card up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hold {
    /// None: the open prompt does not let the card be picked up (`Client`'s `holdable`).
    Never,
    /// Any drag.
    Drag,
    /// A drag that first goes more up or down than sideways, since one that goes sideways
    /// scrolls the card's hand (`rows`).
    UpOrDown,
}

/// What made a press on a card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum By {
    /// The mouse or a pen.
    Mouse,
    /// A finger, whose press that never moved is a tap. It may pick its card up only once
    /// `ready`: a finger on the manual table waits `PICKUP_MS` first, and one that moved before
    /// then never picks the card up.
    Finger { ready: bool },
}

impl Grip {
    /// Moves the press to `at`. A holdable card is picked up once the press has travelled
    /// `DRAG_THRESHOLD_POINTS` from where it began, as the web interface's `Pointer` has it.
    pub fn moved(&mut self, at: Vec2) {
        self.at = at;
        if let Some(sideways) = super::rows::sideways(self.start, at) {
            // Whether the drag scrolls is settled where it first passes the threshold.
            if !self.moved && self.hold == Hold::UpOrDown && sideways {
                self.hold = Hold::Never;
            }
            self.moved = true;
            self.held |= self.hold != Hold::Never && self.by != By::Finger { ready: false };
        }
    }
}

/// A card let go, easing from where it was held to where it rests over `FALL_MS`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Fall {
    pub slot: usize,
    pub from: Pose,
    pub since: u64,
}

impl Fall {
    /// Where the card is at `now`, falling toward `rest`.
    pub fn at(&self, rest: &Pose, now: u64) -> Pose {
        between(&self.from, rest, Curve::EASE_OUT.ease(self.share(now)))
    }

    /// How much of the fall is done at `now`, from 0 to 1.
    fn share(&self, now: u64) -> f32 {
        (now.saturating_sub(self.since) as f32 / (data::FALL_MS * MILLISECOND)).min(1.0)
    }

    pub fn done(&self, now: u64) -> bool {
        self.share(now) >= 1.0
    }
}

/// The lean of a held card: a damped spring, driven by how the pointer moves, that tips the
/// card toward the way it is carried and settles it flat when the pointer stops. It is the one
/// piece of animation state integrated over time instead of computed from it, so it depends
/// only on the pointer's positions and the frames' times.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Lean {
    /// The card's lean, in radians: turned toward +x on the screen, and toward +y (down it).
    tilt: Vec2,
    /// How fast each lean changes, in radians a second.
    spin: Vec2,
    /// The pointer's smoothed velocity, in points a second, and acceleration, in points a second
    /// squared.
    velocity: Vec2,
    acceleration: Vec2,
    /// Where the pointer was and the time of the frame before, once a frame has been drawn
    /// since the card was picked up.
    last: Option<(Vec2, u64)>,
}

impl Lean {
    /// The lean of a card just picked up: flat and still.
    pub const FLAT: Self = Self {
        tilt: Vec2::ZERO,
        spin: Vec2::ZERO,
        velocity: Vec2::ZERO,
        acceleration: Vec2::ZERO,
        last: None,
    };

    /// The lean, in radians, as `tilt` says.
    pub fn tilt(&self) -> Vec2 {
        self.tilt
    }

    /// Advances the spring to the frame at `now`, with the pointer at `at`, in steps of
    /// `1 / LEAN_STEPS_PER_SECOND` second. A gap between frames longer than `MAX_GAP_MS` counts
    /// as that long, as `Heft.advance` has it.
    pub fn advance(&mut self, at: Vec2, now: u64) {
        let Some((was, then)) = self.last.replace((at, now)) else {
            return;
        };
        let gap = (now.saturating_sub(then) as f32 / SECOND).min(data::MAX_GAP_MS / 1000.0);
        if gap <= 0.0 {
            return;
        }
        self.sense(at - was, gap);
        let step = 1.0 / data::LEAN_STEPS_PER_SECOND;
        let mut left = gap;
        while left > 1e-6 {
            self.step(left.min(step));
            left -= step;
        }
    }

    /// Smooths the pointer's velocity and acceleration over the `gap` seconds in which it moved
    /// by `moved`, as `Heft`'s `#sense` does.
    fn sense(&mut self, moved: Vec2, gap: f32) {
        let raw = moved / gap;
        let velocity_share = 1.0 - (-gap * 1000.0 / data::SMOOTH_VELOCITY_MS).exp();
        let acceleration_share = 1.0 - (-gap * 1000.0 / data::SMOOTH_ACCELERATION_MS).exp();
        let velocity = self.velocity + (raw - self.velocity) * velocity_share;
        self.acceleration +=
            ((velocity - self.velocity) / gap - self.acceleration) * acceleration_share;
        self.velocity = velocity;
    }

    /// Advances the spring `dt` seconds, as `Heft`'s `#step` does for the tilt. Held above its
    /// centre, the card lags what moves the grip: it leans into its acceleration and into the
    /// air it pushes through, and the grip's stiffness pulls it back flat. No lean passes
    /// `LEAN_DEGREES`. The held card stays `HELD_HEIGHT_MM` above the table, higher than a lean
    /// that large can dip its edge, so `Heft`'s limit at the felt never applies.
    fn step(&mut self, dt: f32) {
        let omega = TAU * data::STIFFNESS_HZ;
        let damping = 2.0 * data::DAMPING_RATIO * omega;
        let most = data::LEAN_DEGREES.to_radians();
        for axis in 0..2 {
            let pull = self.acceleration[axis] + data::DRAG_PER_SECOND * self.velocity[axis];
            let goal = (pull / data::GRIP_POINTS_PER_SECOND_SQUARED).atan();
            self.spin[axis] +=
                (omega * omega * (goal - self.tilt[axis]) - damping * self.spin[axis]) * dt;
            self.tilt[axis] += self.spin[axis] * dt;
            if self.tilt[axis].abs() > most {
                self.tilt[axis] = most.copysign(self.tilt[axis]);
                self.spin[axis] = 0.0;
            }
        }
    }
}

/// Where a card held with the pointer at `at` is drawn by `camera`, leaning by `tilt`: lying on
/// the plane `HELD_HEIGHT_MM` above the table, its centre under the pointer, at its natural size.
/// A lean toward +x on the screen dips its right edge, and one toward +y (down the screen, toward
/// the viewer) dips its near edge.
pub(super) fn pose(camera: &Camera, at: Vec2, tilt: Vec2) -> Pose {
    Pose {
        position: camera.on_plane(at, data::HELD_HEIGHT_MM),
        rotation: Quat::from_rotation_y(tilt.x) * Quat::from_rotation_x(tilt.y),
        ..Pose::default()
    }
}
