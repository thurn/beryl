//! The card the pointer holds: where it is drawn, the lean of its spring, and its fall back to
//! where it rests once let go (docs/beryl-3d.md, "Motion the pointer drives").
//!
//! The springs use the web interface's `Heft` values (`ui/src/stage/heft.ts`) from
//! `data/beryl3d/pointer.json`. The web measures in stage pixels and the native client
//! measures pointer travel in logical points.

use std::f32::consts::TAU;

use glam::{Quat, Vec2, Vec3};

use game_table::table::Camera;
use kit_base::data::pointer::PointerTuning;
use kit_base::pose::between;
use kit_base::pose::{Curve, Pose};

/// Nanoseconds in a millisecond and in a second.
const MILLISECOND: f32 = 1_000_000.0;
const SECOND: f32 = 1_000_000_000.0;

/// A card let go, easing from where it was held to where it rests over `FALL_MS`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Fall {
    pub slot: usize,
    pub from: Pose,
    pub since: Option<u64>,
}

impl Fall {
    /// Where the card is at `now`, falling toward `rest`.
    pub fn at(&self, rest: &Pose, now: u64, fall_ms: f32) -> Pose {
        between(
            &self.from,
            rest,
            Curve::EASE_OUT.ease(self.share(now, fall_ms)),
        )
    }

    /// How much of the fall is done at `now`, from 0 to 1.
    fn share(&self, now: u64, fall_ms: f32) -> f32 {
        let elapsed = self.since.map_or(0, |since| now.saturating_sub(since));
        (elapsed as f32 / (fall_ms * MILLISECOND)).min(1.0)
    }

    pub fn done(&self, now: u64, fall_ms: f32) -> bool {
        self.share(now, fall_ms) >= 1.0
    }
}

/// A card held under the pointer. A card picked up from the hand keeps its original face
/// height as a fraction of the window, with the small increase from the web's lift spring.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Held {
    pub slot: usize,
    pub at: Vec2,
    pub hand_height: Option<f32>,
}

impl Held {
    /// Draws a held hand card facing the camera, so carrying it towards the opponent does
    /// not shrink or foreshorten it. Drags from other zones lie on the play surface's plane.
    pub fn pose(self, camera: &Camera, heft: &Heft, tuning: &PointerTuning) -> Pose {
        let height = tuning.held_height_mm;
        let position = camera.on_plane(self.at, height);
        let tilt = heft.tilt();
        let Some(hand_height) = self.hand_height else {
            return Pose {
                position,
                rotation: Quat::from_rotation_y(tilt.x) * Quat::from_rotation_x(tilt.y),
                ..Pose::default()
            };
        };

        let ndc = camera.window.size.ndc(self.at.x, self.at.y);
        let rotation = Quat::from_rotation_x(tilt.y) * Quat::from_rotation_y(tilt.x);
        let face_height = hand_height * (1.0 + heft.lift * (tuning.held_hand_scale - 1.0));
        let mut pose = camera.screen_pose(ndc, camera.depth(position), rotation, face_height);
        // At a steep angle the lower corner can reach the felt. Raise the card along the
        // pointer ray and keep the same projected size.
        let half = camera.data.card.half();
        let down =
            (pose.rotation * Vec3::X).z.abs() * half.x + (pose.rotation * Vec3::Y).z.abs() * half.y;
        let clearance = down * pose.scale;
        if clearance > height {
            let depth = camera.depth(camera.on_plane(self.at, clearance));
            pose = camera.screen_pose(ndc, depth, rotation, face_height);
        }
        pose
    }
}

/// Pointer motion drives a damped spring that leans the card and settles it flat when the
/// pointer stops. A separate critically damped spring grows a lifted hand card.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Heft {
    /// The card's lean, in radians: turned toward +x on the screen, and toward +y (down it).
    tilt: Vec2,
    /// How fast each lean changes, in radians a second.
    spin: Vec2,
    /// The pointer's smoothed velocity, in points a second, and acceleration, in points a second
    /// squared.
    velocity: Vec2,
    acceleration: Vec2,
    /// The hand card's progress from pickup size to fully lifted size, and its speed.
    lift: f32,
    rise: f32,
    /// Where the pointer was and the time of the frame before, once a frame has been drawn
    /// since the card was picked up.
    last: Option<(Vec2, u64)>,
}

impl Heft {
    /// The lean of a card just picked up: flat and still.
    pub const FLAT: Self = Self {
        tilt: Vec2::ZERO,
        spin: Vec2::ZERO,
        velocity: Vec2::ZERO,
        acceleration: Vec2::ZERO,
        lift: 0.0,
        rise: 0.0,
        last: None,
    };

    /// The lean, in radians, as `tilt` says.
    pub fn tilt(&self) -> Vec2 {
        self.tilt
    }

    /// Advances the springs to the frame at `now` with the card at the pointer, in steps of
    /// `1 / LEAN_STEPS_PER_SECOND` second. A gap between frames longer than `MAX_GAP_MS` counts
    /// as that long, as `Heft.advance` has it.
    pub fn advance(&mut self, held: Held, now: u64, tuning: &PointerTuning) {
        let Some((was, then)) = self.last.replace((held.at, now)) else {
            return;
        };
        let gap = (now.saturating_sub(then) as f32 / SECOND).min(tuning.max_gap_ms / 1000.0);
        if gap <= 0.0 {
            return;
        }
        self.sense(held.at - was, gap, tuning);
        let step = 1.0 / tuning.lean_steps_per_second;
        let mut left = gap;
        while left > 1e-6 {
            let dt = left.min(step);
            if held.hand_height.is_some() {
                let omega = TAU * tuning.held_hand_lift_hz;
                self.rise += (omega * omega * (1.0 - self.lift) - 2.0 * omega * self.rise) * dt;
                self.lift += self.rise * dt;
            }
            self.step(dt, tuning);
            left -= step;
        }
    }

    /// Smooths the pointer's velocity and acceleration over the `gap` seconds in which it moved
    /// by `moved`, as `Heft`'s `#sense` does.
    fn sense(&mut self, moved: Vec2, gap: f32, tuning: &PointerTuning) {
        let raw = moved / gap;
        let velocity_share = 1.0 - (-gap * 1000.0 / tuning.smooth_velocity_ms).exp();
        let acceleration_share = 1.0 - (-gap * 1000.0 / tuning.smooth_acceleration_ms).exp();
        let velocity = self.velocity + (raw - self.velocity) * velocity_share;
        self.acceleration +=
            ((velocity - self.velocity) / gap - self.acceleration) * acceleration_share;
        self.velocity = velocity;
    }

    /// Advances the spring `dt` seconds, as `Heft`'s `#step` does for the tilt. Held above its
    /// centre, the card lags what moves the grip: it leans into its acceleration and into the
    /// air it pushes through, and the grip's stiffness pulls it back flat. No lean passes
    /// the tuning's maximum angle. `Held::pose` keeps the face above the felt.
    fn step(&mut self, dt: f32, tuning: &PointerTuning) {
        let omega = TAU * tuning.stiffness_hz;
        let damping = 2.0 * tuning.damping_ratio * omega;
        let most = tuning.lean_degrees.to_radians();
        for axis in 0..2 {
            let pull = self.acceleration[axis] + tuning.drag_per_second * self.velocity[axis];
            let goal = (pull / tuning.grip_points_per_second_squared).atan();
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
