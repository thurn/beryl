//! The prompt shape `Declare`: objects each aimed at something, the attackers and what each
//! attacks, or the blockers and the attacker each blocks.
//!
//! A creature is clicked where it lies (`Client::frame` sends `Click::On` for it), and a player
//! through their life total while the prompt marks them (`CountFloats`): the session takes a
//! click on a creature as declaring it or taking it back, and one on what creatures awaiting
//! their aim can be aimed at as aiming them, and its reply brings the declaration as it now
//! stands. What can be declared, what is and what can be aimed at wear the marks the session
//! sends. Each creature declared and aimed gets a line to what it is aimed at, where the
//! session says lines are drawn (`lined`), which the client begins and ends itself (`own`), as
//! the web interface's `#mark` in `ui/src/stage/prompt.ts` does. While creatures await their
//! aim, a line runs from each to the mouse (`Client::set_aims`), as the web's `Aim`
//! (`ui/src/stage/Aim.tsx`) draws it. The buttons are the session's, in its order: All Attack
//! declares every creature that can attack, and the decline takes the declaration back or
//! declares none.

use beryl_protocol::prompt::{PromptView, ShapeBody};
use beryl_protocol::script::Subject;

use super::{Shown, Steps, begin_spans, end_spans, mark_spans, prompt_bar};
use crate::element::Element;
use crate::light::{ArrowSpan, Span};
use crate::screens::ScreenIn;

/// What the flat layer shows for `prompt`: the prompt bar. A player the prompt marks is aimed
/// at by a press on their life total (`CountFloats`).
#[expect(
    clippy::unnecessary_wraps,
    reason = "every shape returns an `Option`: a built shape may show nothing in the flat layer"
)]
pub(super) fn screen(prompt: &PromptView, input: &ScreenIn) -> Option<Element> {
    Some(prompt_bar(prompt, input))
}

/// Makes what the table shows for the declaration `shown` replace what it showed for `was`, as
/// one beat: the marks and lines `was` drew and `shown` does not end, then those new in `shown`
/// begin. Allocates nothing.
pub(super) fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    end_spans(spans, shown, was, steps);
    begin_spans(spans, shown, was, steps);
}

/// The spans the table draws for `prompt`, as `Prompts.#mark` does: each of its marks, and,
/// where its lines are drawn, a line from each creature declared to what it is aimed at, but
/// none to a player: Arena's attacker at a player shows only its chevron
/// (`IEXcjBn3BXk 22:16.53`), as the script that declares it draws no line either.
fn spans(prompt: Option<Shown<'_>>) -> impl Iterator<Item = Span> + '_ {
    prompt.into_iter().flat_map(|shown| {
        let lines = match &shown.prompt.body {
            ShapeBody::Declare {
                arrow,
                pairs,
                lined: true,
                ..
            } => Some((*arrow, pairs)),
            _ => None,
        };
        let lines = lines.into_iter().flat_map(|(arrow, pairs)| {
            pairs
                .iter()
                .filter(|pair| !matches!(pair.to, Subject::Player { .. }))
                .map(move |pair| {
                    Span::Arrow(ArrowSpan {
                        arrow,
                        from: Subject::Object { slot: pair.from },
                        to: pair.to,
                    })
                })
        });
        mark_spans(shown.prompt).chain(lines)
    })
}
