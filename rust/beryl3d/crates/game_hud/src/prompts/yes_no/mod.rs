//! Yes or no, including questions that preview what paying for yes would tap.

mod view;

use super::{Shown, Steps, marks};
use beryl_protocol::prompt::{ButtonId, Marked, PromptView, ShapeBody};
use beryl_protocol::script::Mark;

/// Whether `button` of the yes or no `prompt` shows what it would tap: its primary button, the
/// yes, where the question pays a cost and marks what paying it would tap (`suggested`), as the
/// web's `asksToPay` has it.
pub(super) fn shows(prompt: &PromptView, button: ButtonId) -> bool {
    let pays = (prompt.marks.iter()).any(|marked| marked.mark == Mark::Suggested);
    pays && (prompt.buttons.iter()).any(|view| view.id == button && view.primary)
}

/// Begins and ends what the table shows for the prompt `shown`, which was `was`: the tray
/// holds up what it is about, and a question whose yes pays draws what paying would tap only
/// while the pointer is on the yes, as the web stage's `data-pay="asking"` rules in
/// `ui/src/stage/stage.css` have it. Every other mark is drawn as the session sends it, a
/// number's marks on what its value would tap among them.
pub(super) fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    marks(shown, was, &look, steps);
}

/// The mark the table draws for `marked` while the prompt is `shown`.
fn look(shown: Shown, marked: Marked) -> Option<Mark> {
    let asking = matches!(shown.prompt.body, ShapeBody::YesNo { .. });
    match marked.mark {
        Mark::Suggested if asking => shown.suggesting.then_some(Mark::Suggested),
        mark => Some(mark),
    }
}

prompt_feature!(YesNo, view::view, own);
