//! `dwell`: a spell or ability on the stack is left to be read, or the table to be seen as the
//! game ends. Nothing moves, and the step holds its beat for its duration. Its subject is drawn
//! `focused` while the step plays, so that the tree shows what is being read: the step begins a
//! `Span::Focus` as it starts and ends it as it ends.

use beryl_protocol::script::{Act, Span as Edge};

use super::{Interval, StepIn, StepOut};
use crate::light::{Span, SpanChange};

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    let Act::Dwell { subject } = *step.act else {
        return;
    };
    let own = step.own;
    for (end, at) in [(Edge::Begin, own.from), (Edge::End, own.to)] {
        out.span(SpanChange {
            span: Span::Focus(subject),
            end,
            over: Interval { from: at, to: at },
            step: step.number,
        });
    }
}
