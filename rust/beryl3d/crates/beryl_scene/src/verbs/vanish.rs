//! `vanish`: the card or ability goes from where it is with no flight. It stays where it was
//! for the whole step, fading as the web interface's `dissolve` does (`Shape::Fading`), and
//! at the step's end it is at its new place, or gone when the act leaves it none. An ability
//! that finishes has a step of no duration and is cut at once. The look of its going (ash,
//! white light, a glow) is the step's effect, drawn apart from this motion.

use super::{StepIn, StepOut};
use crate::pose::{Segment, Shape};

pub const BUILT: bool = true;

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
