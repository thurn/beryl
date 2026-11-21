//! `vanish`: the card or ability goes from where it is with no flight. It stays where it was
//! for the whole step, fading as the web interface's `dissolve` does (`Shape::Fading`), and
//! at the step's end it is at its new place, or gone when the act leaves it none. An ability
//! that finishes has a step of no duration and is cut at once. The look of its going (ash,
//! white light, a glow) is the step's effect, drawn apart from this motion.

use super::{StepIn, StepOut};
use kit_base::pose::{Segment, Shape};

pub const BUILT: bool = true;

/// How long what a card that vanishes made room for takes to close up once it has gone, as a
/// share of the step that took it, where its timing names no time of its own
/// (`StepTiming::closes`): the row closes up only after the card is gone. The web interface's
/// `CLOSES` and `roomIn` in `ui/src/stage/tracks.ts`.
pub const CLOSES: f64 = 0.4;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let (Some(subject), Some(from), Some(to)) = (step.subject, step.from, step.to) else {
        return;
    };
    out.segment(
        subject,
        Segment {
            shape: Shape::Fading,
            ..Segment::between(from, to, step.own, step.curve)
        },
    );
}
