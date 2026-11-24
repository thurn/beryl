//! `mark`: a mark on a card, an ability or a player begins or ends. It moves nothing: the step
//! writes a `SpanChange`, which reaches the table's `Lit` at the step's real time, and
//! `light::marks` draws the mark from there until the step that ends it.

use super::{StepIn, StepOut};
use crate::light::{MarkSpan, Span, SpanChange};
use beryl_protocol::script::Act;

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let Act::Mark { span, mark, on } = *step.act else {
        return;
    };
    out.span(SpanChange {
        span: Span::Mark(MarkSpan { mark, on }),
        end: span,
        over: step.own,
        step: step.number,
    });
}
