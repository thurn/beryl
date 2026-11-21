//! The divide prompt and the steps it makes on the table.

mod view;

use super::{Shown, Steps};
use super::{begin_spans, end_spans, mark_spans, target_arrows};
use game_play::light::Span;

/// Makes what the table shows for the division `shown` replace what it showed for `was`, as one
/// beat: the marks and arrows `was` drew and `shown` does not end, then those new in `shown`
/// begin. Allocates nothing.
pub(super) fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    end_spans(spans, shown, was, steps);
    begin_spans(spans, shown, was, steps);
}

/// The spans the table draws for `prompt`, as `Prompts.#mark` does: each of its marks, and an
/// arrow to each recipient from the spell or ability that asks (`target_arrows`).
fn spans(prompt: Option<Shown<'_>>) -> impl Iterator<Item = Span> + '_ {
    prompt
        .into_iter()
        .flat_map(|shown| mark_spans(shown.prompt).chain(target_arrows(shown)))
}

prompt_feature!(Divide, view::view, own, view::details);
