//! `lift`: as the opponent begins to look at cards (`Span::Begin`), the cards they handle come
//! away from their pile to where the table holds them up while they are handled
//! (`table::lifted`), face down where they come from a library; as the look ends they go back.
//! A look at several cards of a library takes them one after another, each as long as one alone
//! (`StepTiming::each`, `stagger`), the top card first. The look itself is a span the table shows
//! from the step that begins it to the step that ends it (`Span::Lift`).

use beryl_protocol::script::{Act, Subject};

use super::{Interval, StepIn, StepOut};
use crate::light::{Span, SpanChange};
use crate::play::nanoseconds;
use kit_base::pose::Segment;

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let Act::Lift { span, handling } = step.act else {
        return;
    };
    out.span(SpanChange {
        span: Span::Lift(*handling),
        end: *span,
        over: step.own,
        step: step.number,
    });
    let timing = step.timing;
    for (index, &slot) in step.places.handled().iter().enumerate() {
        let subject = Subject::Object { slot };
        let (Some(from), Some(to)) = (
            step.places.pose(subject, step.own.from),
            step.places.rest(subject),
        ) else {
            continue;
        };
        let over = match timing.each {
            Some(each) => {
                #[expect(clippy::cast_precision_loss, reason = "a handful of cards")]
                let from = step.own.from + nanoseconds(index as f64 * timing.stagger);
                Interval {
                    from,
                    to: from + nanoseconds(each),
                }
            }
            None => step.own,
        };
        // Where it was as its own part of the step begins: it waits on its pile until then.
        let from = step.places.pose(subject, over.from).unwrap_or(from);
        out.segment(subject, Segment::between(from, to, over, step.curve));
    }
}
