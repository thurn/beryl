//! `flip`: the card turns over where it lies, its `Pose::turn` running from the side it showed
//! as the step began to the side its record now shows, rising and growing while it turns
//! (`Shape::TurnedOver`, with `verbs.flip` of `data/beryl3d/space.json`). The veil that hides the
//! change is the `cover` effect's, which the step's timing begins.

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
            shape: Shape::TurnedOver,
            ..Segment::between(from, to, step.own, step.curve)
        },
    );
}
