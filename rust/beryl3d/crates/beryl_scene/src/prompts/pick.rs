//! The prompt shape `Pick`, objects picked one click at a time, and a prompt the session holds
//! no state for (`Candidates`), answered by the decision's candidates.
//!
//! A pick's candidates are clicked where they lie: a card on the table or in a hand through the
//! pointer (`Client::frame` sends `Click::On` for it), and a player through their life total,
//! which takes the press while the prompt marks them (`CountFloats`), as the web interface's
//! `PlayerTarget` (`ui/src/stage/PromptBar.tsx`) does. What may be picked and what is picked
//! wear the marks the session sends, and each target picked gets an arrow from the spell or
//! ability that asks, which the client begins and ends itself (`own`). Cards a pick lays out
//! off the table, such as the top of a library searched, are held up in the tray while it is
//! open, as the web's `trayOf` (`ui/src/stage/prompt.ts`) holds a pick's `tray`.
//!
//! A prompt the session holds no state for is not the table's to show, as the web's `onTable`
//! has it: it shows the prompt bar and draws nothing on the table.

use beryl_protocol::prompt::{PromptView, ShapeBody};
use beryl_protocol::script::{Mark, Subject};

use super::{Shown, Steps, mark_spans, prompt_bar, target_arrows, tray};
use crate::hud::HudIn;
use crate::light::{MarkSpan, Span};
use kit_ui::element::Element;

/// What the flat layer shows for `prompt`: the prompt bar. A player the prompt marks is picked
/// by a press on their life total (`CountFloats`).
#[expect(
    clippy::unnecessary_wraps,
    reason = "every shape returns an `Option`: a built shape may show nothing in the flat layer"
)]
pub(super) fn screen(prompt: &PromptView, input: &HudIn) -> Option<Element> {
    Some(prompt_bar(prompt, input))
}

/// Makes what the table shows for the prompt `shown` replace what it showed for `was`, as one
/// beat (`tray::own`): its marks and arrows, and the cards its tray holds up.
pub(super) fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    tray::own(shown, was, spans, steps);
}

/// The spans the table draws for `prompt`, as `Prompts.#mark` does: each of its marks; an arrow
/// to each target picked (`target_arrows`); and a suggested mark on each permanent that paying
/// for the card the pointer carries out of the hand would tap and nothing else of the prompt
/// marks, as Arena outlines the lands a spell held out of the hand would tap
/// (`iRr-GeSF0lI 20:34.4`). A prompt the session holds no state for draws none.
fn spans(prompt: Option<Shown<'_>>) -> impl Iterator<Item = Span> + '_ {
    let shown = prompt.filter(|shown| matches!(shown.prompt.body, ShapeBody::Pick { .. }));
    shown.into_iter().flat_map(|shown| {
        let marks = mark_spans(shown.prompt);
        let arrows = target_arrows(shown);
        let carried = (shown.prompt.previews.iter())
            .filter(move |preview| Some(preview.card) == shown.carried)
            .flat_map(|preview| preview.taps.iter())
            .map(|&slot| Subject::Object { slot })
            .filter(move |on| !shown.prompt.marks.iter().any(|marked| marked.on == *on))
            .map(|on| {
                Span::Mark(MarkSpan {
                    mark: Mark::Suggested,
                    on,
                })
            });
        marks.chain(arrows).chain(carried)
    })
}
