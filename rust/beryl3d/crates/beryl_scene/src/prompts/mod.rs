//! Prompts: what the flat layer shows for the open prompt, and the marks and arrows the
//! client begins and ends itself while the player answers it (docs/beryl-3d.md, "The flat
//! layer"). Each group of shapes has a file of its own; both entry points are one match over
//! the prompt's `ShapeBody`.

mod bar;
mod catalog;
mod choose;
mod declare;
mod divide;
mod order;
mod pay;
mod pick;
mod split;

use beryl_protocol::prompt::{PromptView, ShapeBody};
use beryl_protocol::script::OwnTimings;

pub use bar::prompt_bar;

use crate::element::Element;
use crate::light::Lit;
use crate::screens::ScreenIn;

/// What the flat layer shows for `prompt`, if anything.
#[must_use]
pub fn screen(prompt: &PromptView, input: &ScreenIn) -> Option<Element> {
    match &prompt.body {
        ShapeBody::Pick { .. } | ShapeBody::Candidates => pick::screen(prompt, input),
        ShapeBody::Split { .. } => split::screen(prompt, input),
        ShapeBody::Order { .. } => order::screen(prompt, input),
        ShapeBody::Pay { .. } => pay::screen(prompt, input),
        ShapeBody::Declare { .. } => declare::screen(prompt, input),
        ShapeBody::YesNo { .. } | ShapeBody::Number { .. } | ShapeBody::Mode { .. } => {
            choose::screen(prompt, input)
        }
        ShapeBody::Catalog { .. } => catalog::screen(prompt, input),
        ShapeBody::Divide { .. } => divide::screen(prompt, input),
    }
}

/// Begins and ends the marks and arrows on the table that the client draws itself for
/// `prompt`, which was `was` before (none when it has just opened), timed by `timings` from
/// `now`, in nanoseconds since the client started.
pub fn own(
    prompt: &PromptView,
    was: Option<&PromptView>,
    lit: &mut Lit,
    timings: &OwnTimings,
    now: u64,
) {
    match &prompt.body {
        ShapeBody::Pick { .. } | ShapeBody::Candidates => {
            pick::own(prompt, was, lit, timings, now);
        }
        ShapeBody::Split { .. } => split::own(prompt, was, lit, timings, now),
        ShapeBody::Order { .. } => order::own(prompt, was, lit, timings, now),
        ShapeBody::Pay { .. } => pay::own(prompt, was, lit, timings, now),
        ShapeBody::Declare { .. } => declare::own(prompt, was, lit, timings, now),
        ShapeBody::YesNo { .. } | ShapeBody::Number { .. } | ShapeBody::Mode { .. } => {
            choose::own(prompt, was, lit, timings, now);
        }
        ShapeBody::Catalog { .. } => catalog::own(prompt, was, lit, timings, now),
        ShapeBody::Divide { .. } => divide::own(prompt, was, lit, timings, now),
    }
}
