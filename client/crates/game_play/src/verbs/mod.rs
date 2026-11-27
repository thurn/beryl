//! The verbs: each turns one step of a script into what the table shows for it
//! (docs/beryl-3d.md, "Verbs"). Each of the 16 verbs is a file of this module with one function,
//! `play`, which is given the step as a `StepIn` (where its subject is as the step begins, where
//! it rests once the step is done, when the step plays) and writes what it shows into a
//! `StepOut`: segments on scene objects, spans that begin or end, streaks and notices. Every
//! time comes from the step's `StepTiming`, through `beat`, so a verb reads no duration, delay or
//! wait of its own.
//!
//! A verb whose `BUILT` is false is drawn as an instant change: its subject is at its new place
//! as the step begins, and the step still holds its beat for as long as its timing says.

mod announce;
mod appear;
mod count;
mod dwell;
mod emerge;
mod flip;
pub(crate) mod fly;
mod lift;
mod mark;
mod point;
mod set;
mod showcase;
mod streak;
mod strike;
mod tap;
pub(crate) mod vanish;

use beryl_protocol::script::Verb;

pub use crate::play::step::{Places, StepIn, StepOut};
use kit_base::pose::{Curve, Pose, Segment};
pub use kit_base::pose::{Follows, Interval, StepTime};

/// The function that plays `verb`'s steps.
#[must_use]
pub fn play(verb: Verb) -> fn(&StepIn<'_>, &mut StepOut<'_>) {
    match verb {
        Verb::Fly => fly::play,
        Verb::Appear => appear::play,
        Verb::Vanish => vanish::play,
        Verb::Emerge => emerge::play,
        Verb::Flip => flip::play,
        Verb::Tap => tap::play,
        Verb::Strike => strike::play,
        Verb::Streak => streak::play,
        Verb::Count => count::play,
        Verb::Set => set::play,
        Verb::Showcase => showcase::play,
        Verb::Dwell => dwell::play,
        Verb::Announce => announce::play,
        Verb::Point => point::play,
        Verb::Mark => mark::play,
        Verb::Lift => lift::play,
    }
}

/// Whether `verb` is drawn as motion of its own, or as an instant change.
#[must_use]
pub const fn built(verb: Verb) -> bool {
    match verb {
        Verb::Fly => fly::BUILT,
        Verb::Appear => appear::BUILT,
        Verb::Vanish => vanish::BUILT,
        Verb::Emerge => emerge::BUILT,
        Verb::Flip => flip::BUILT,
        Verb::Tap => tap::BUILT,
        Verb::Strike => strike::BUILT,
        Verb::Streak => streak::BUILT,
        Verb::Count => count::BUILT,
        Verb::Set => set::BUILT,
        Verb::Showcase => showcase::BUILT,
        Verb::Dwell => dwell::BUILT,
        Verb::Announce => announce::BUILT,
        Verb::Point => point::BUILT,
        Verb::Mark => mark::BUILT,
        Verb::Lift => lift::BUILT,
    }
}

/// An instant change at `at`: the object stands at `from` until then and at `to` from then on.
#[must_use]
pub const fn instant(from: Pose, to: Pose, at: u64) -> Segment {
    Segment::between(from, to, Interval { from: at, to: at }, Curve::LINEAR)
}

/// The objects that no step names but whose place a step changed (the rest of a hand closing up
/// round a card that left it) slide from where they are to where they now rest, over `interval`
/// (the one the step's `StepTiming::room` gives), slowing into place.
#[must_use]
pub const fn slide(from: Pose, to: Pose, interval: Interval) -> Segment {
    Segment::between(from, to, interval, Curve::EASE_OUT)
}
