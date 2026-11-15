//! `showcase`: the card stays where it lies, and its face joins the Revealed Cards pile at the top
//! left of the screen, growing into its place there over the step (`Shape::Grown`). The face is a
//! span the table shows until a script with a reset, or until another cause reveals cards
//! (`Span::Revealed`); the table draws it from the record the step's act holds
//! (`Table::reveal`).

use beryl_protocol::script::{Act, Span as Edge};

use super::{StepIn, StepOut};
use crate::light::{Span, SpanChange};

pub const BUILT: bool = true;

pub fn play(step: &StepIn<'_>, out: &mut StepOut<'_>) {
    // The card's own record may have changed: the face it shows, now known.
    out.instant();
    let Act::Showcase { object } = step.act else {
        return;
    };
    out.span(SpanChange {
        span: Span::Revealed {
            slot: object.slot,
            cause: step.cause,
        },
        end: Edge::Begin,
        over: step.own,
        step: step.number,
    });
}
