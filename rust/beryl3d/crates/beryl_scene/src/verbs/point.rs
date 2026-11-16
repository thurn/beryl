//! `point`: an arrow or line between two things begins or ends. The step that begins it has the
//! arrow fade in over the step, and the step that ends it has it fade out over the step; from one
//! to the other it is shown, across scripts, and drawn on every frame from where its two ends are
//! then (`light::arrows`). It moves nothing.

use beryl_protocol::script::Act;

use super::{StepIn, StepOut};
use crate::light::{ArrowSpan, Span, SpanChange};

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let Act::Point {
        span,
        arrow,
        from,
        to,
    } = step.act
    else {
        return;
    };
    out.span(SpanChange {
        span: Span::Arrow(ArrowSpan {
            arrow: *arrow,
            from: *from,
            to: *to,
        }),
        end: *span,
        over: step.own,
        step: step.number,
    });
}
