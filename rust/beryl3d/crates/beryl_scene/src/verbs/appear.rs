//! `appear`: the object comes to be at its place with no flight, fading in where it rests.

use super::{StepIn, StepOut};
use crate::pose::{Pose, Segment};

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let (Some(subject), Some(to)) = (step.subject, step.to) else {
        return;
    };
    let from = Pose { opacity: 0.0, ..to };
    out.segment(subject, Segment::between(from, to, step.own, step.curve));
}
