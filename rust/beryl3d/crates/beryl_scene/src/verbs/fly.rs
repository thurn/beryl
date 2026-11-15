//! `fly`: the card travels from where it is to where it rests, its path rising `verbs.fly.arc`
//! millimetres (`data/beryl3d/space.json`) above the straight line at the middle of its time,
//! and turning over on the way if its face changes.

use super::{StepIn, StepOut};
use crate::data::space;
use crate::pose::Segment;

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let (Some(subject), Some(from), Some(to)) = (step.subject, step.from, step.to) else {
        return;
    };
    out.segment(
        subject,
        Segment {
            arc: space::verbs::fly::ARC,
            ..Segment::between(from, to, step.own, step.curve)
        },
    );
}
