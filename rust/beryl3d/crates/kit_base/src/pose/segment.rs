//! How a motion segment changes a pose over time.

use std::f32::consts::PI;

use glam::{Quat, Vec3};

use super::{Curve, Pose, Segment, Shape};
use crate::data::verbs::{Flip, Thrown};

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
fn thrown_height(elapsed: f32, tuning: Thrown) -> f32 {
    let at = elapsed.clamp(0.0, 1.0);
    if at < tuning.risen {
        1.0 - (1.0 - at / tuning.risen).powi(2)
    } else if at <= tuning.held {
        1.0
    } else {
        1.0 - (at - tuning.held) / (1.0 - tuning.held)
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
            Shape::Thrown { tuning, .. } => Some(at(tuning.risen)),
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
            Shape::Thrown {
                rise: up, tuning, ..
            } => Along {
                u: 1.0 - (1.0 - eased).powf(tuning.launch),
                offset: rise + up * thrown_height(elapsed, tuning),
                lean: Quat::IDENTITY,
                grow: 0.0,
            },
            Shape::Swept { bulge, grow, .. } => {
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
    /// first scale of its way down within `tuning.shrunk` of its time, then down to its end
    /// pose's.
    fn thrown_scale(&self, aloft: f32, elapsed: f32, tuning: Thrown) -> f32 {
        let (from, to) = (self.from.scale, self.to.scale);
        if elapsed < tuning.shrunk {
            let first = thrown_down(aloft, to, tuning.shrunk);
            from + (first - from) * (elapsed / tuning.shrunk)
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
        let pose = self.unlifted(elapsed);
        // A lifted card is drawn nearer the eye along its line of sight, and smaller by as
        // much, so that on screen it is where the straight way has it.
        match self.shape.lift() {
            Some(lift) => {
                let nearer = lift.nearer(pose.position, self.along(elapsed).u);
                Pose {
                    position: lift.eye + (pose.position - lift.eye) * nearer,
                    scale: pose.scale * nearer,
                    ..pose
                }
            }
            None => pose,
        }
    }

    /// A travelling path's pose `elapsed` of its time through, where it would be drawn if its
    /// shape held it no nearer the eye.
    fn unlifted(&self, elapsed: f32) -> Pose {
        let Along {
            u,
            offset,
            lean,
            grow,
        } = self.along(elapsed);
        let pose = between(&self.from, &self.to, u);
        let scale = match self.shape {
            Shape::Thrown { aloft, tuning, .. } => self.thrown_scale(aloft, elapsed, tuning),
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
        Pose {
            position: pose.position + offset,
            rotation: lean * pose.rotation,
            scale,
            turn: self.from.turn + (self.to.turn - self.from.turn) * turned,
            ..pose
        }
    }

    /// Where its card lies on its way at `t`, as `pose_at` has it but before a shape that holds
    /// the card nearer the eye moves it along its line of sight (`Shape::lift`): the same place
    /// on the window, at the depth its way has.
    #[must_use]
    pub fn unlifted_at(&self, t: u64) -> Pose {
        if self.shape.lift().is_none() || t <= self.start || t >= self.end() {
            return self.pose_at(t);
        }
        self.unlifted(self.elapsed(t))
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
            Shape::Regrouping(fade) => Pose {
                opacity: fade.opacity(t - self.start),
                ..self.travelled(elapsed)
            },
            Shape::TurnedOver(tuning) => turned_over(&self.from, &self.to, elapsed, tuning),
            Shape::Grown(tuning) => {
                let u = self.curve.ease(elapsed);
                let pose = between(&self.from, &self.to, u);
                let size = tuning.size;
                Pose {
                    scale: pose.scale * (size + (1.0 - size) * u),
                    opacity: pose.opacity * (u / tuning.seen).min(1.0),
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
pub fn between(from: &Pose, to: &Pose, u: f32) -> Pose {
    Pose {
        position: from.position.lerp(to.position, u),
        rotation: from.rotation.slerp(to.rotation, u),
        scale: from.scale + (to.scale - from.scale) * u,
        opacity: from.opacity + (to.opacity - from.opacity) * u,
        turn: from.turn + (to.turn - from.turn) * u,
        skew: from.skew + (to.skew - from.skew) * u,
        shear: from.shear + (to.shear - from.shear) * u,
    }
}

/// A card turning over where it lies, `elapsed` of the way through (`Shape::TurnedOver`), as
/// the web interface's `turnedOver` (`ui/src/stage/tracks.ts`) has it: it rises and grows
/// quickly and then slowing into its largest by `grown`, stays up, and from `settle` comes back
/// down, slowing onto the table. It turns over while it is up, evenly from `grown` to `settle`,
/// so that it is edge-on, and shows its new side, at the middle of its time.
fn turned_over(from: &Pose, to: &Pose, elapsed: f32, tuning: Flip) -> Pose {
    let (grown, settle) = (tuning.grown, tuning.settle);
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
        position: pose.position + Vec3::Z * tuning.rise * up,
        scale: pose.scale * (1.0 + (tuning.scale - 1.0) * up),
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
        let pose = match self.shape.lift() {
            Some(lift) => lift.unlifted(pose, u),
            None => pose,
        };
        let to = self.to;
        // Each part of the pose is `from + (to - from) * u`, so `from = (now - to * u) / rest`.
        let back = |now: f32, to: f32| (now - to * u) / rest;
        // `slerp(from, to, u)` is `to * (to⁻¹ * from)^(1 - u)`, so `to⁻¹ * from` is
        // `(to⁻¹ * now)^(1 / rest)`, where `now` is the pose's rotation before its lean.
        let unleaned = lean.inverse() * pose.rotation;
        let (axis, angle) = (to.rotation.inverse() * unleaned).to_axis_angle();
        let grown = self.held_grown(elapsed);
        let scale = match &mut self.shape {
            Shape::Thrown { aloft, tuning, .. } if elapsed < tuning.shrunk => {
                let share = elapsed / tuning.shrunk;
                let first = thrown_down(*aloft, to.scale, tuning.shrunk);
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
            skew: back(pose.skew, to.skew),
            shear: back(pose.shear, to.shear),
        };
    }
}
