//! The verbs: each turns one step of a script into a segment on the scene object it concerns
//! (docs/beryl-3d.md, "Verbs"). A verb is given where the object is when its step begins, where
//! the object rests once the step is done, and when the step begins and ends as the beat lays it
//! out. Every time comes from the step's `StepTiming`, through `beat`, so a verb reads no
//! duration, delay or wait of its own.
//!
//! `fly`, `appear` and `set` are built. Every other verb is an instant change: the object is at
//! its new place as the step begins, and the step still holds its beat for as long as its timing
//! says.

use beryl_session::script::Verb;

use crate::data::space;
use crate::pose::{Curve, Pose, Segment};

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

/// Whether the verb is drawn as motion of its own, or as an instant change.
#[must_use]
pub const fn built(verb: Verb) -> bool {
    matches!(verb, Verb::Fly | Verb::Appear | Verb::Set)
}

/// The segment that shows `verb` moving an object from `from`, where it is as the step begins,
/// to `to`, where it rests once the step is done, over `interval` along `curve`.
#[must_use]
pub fn segment(verb: Verb, from: Pose, to: Pose, interval: Interval, curve: Curve) -> Segment {
    match verb {
        Verb::Fly => fly(from, to, interval, curve),
        Verb::Appear => appear(to, interval, curve),
        _ => instant(from, to, interval.from),
    }
}

/// `fly`: the card travels from where it is to where it rests, its path rising
/// `verbs.fly.arc` millimetres (`data/beryl3d/space.json`) above the straight line at the middle
/// of its time, and turning over on the way if its face changes.
#[must_use]
pub fn fly(from: Pose, to: Pose, interval: Interval, curve: Curve) -> Segment {
    Segment {
        start: interval.from,
        duration: interval.duration(),
        curve,
        from,
        to,
        arc: space::verbs::fly::ARC,
    }
}

/// `appear`: the object comes to be at its place with no flight, fading in where it rests.
#[must_use]
pub fn appear(to: Pose, interval: Interval, curve: Curve) -> Segment {
    Segment {
        start: interval.from,
        duration: interval.duration(),
        curve,
        from: Pose { opacity: 0.0, ..to },
        to,
        arc: 0.0,
    }
}

/// An instant change at `at`: the object stands at `from` until then and at `to` from then on.
/// `set` is one ("a part of the table is now this, and nothing moves"), and so is every verb not
/// yet built.
#[must_use]
pub fn instant(from: Pose, to: Pose, at: u64) -> Segment {
    Segment {
        start: at,
        duration: 0,
        curve: Curve::LINEAR,
        from,
        to,
        arc: 0.0,
    }
}

/// The objects that no step names but whose place a step changed (the rest of a hand closing up
/// round a card that left it) slide from where they are to where they now rest, over `interval`,
/// slowing into place.
#[must_use]
pub fn slide(from: Pose, to: Pose, interval: Interval) -> Segment {
    Segment {
        start: interval.from,
        duration: interval.duration(),
        curve: Curve::EASE_OUT,
        from,
        to,
        arc: 0.0,
    }
}
