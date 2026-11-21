//! The prompt shape `Pay`: a cost paid with permanents, such as mana, a crew cost or a
//! waterbend cost (UI Conventions, "Paying with permanents"). The prompt bar says how much is
//! still to pay, and the table marks what Auto Pay would tap only while the pointer is on Auto
//! Pay: until then those permanents are outlined as anything else that may be picked. A click
//! on a permanent where it lies picks it or takes it back, through the table's pointer.

use beryl_protocol::prompt::{ButtonId, Marked};
use beryl_protocol::script::Mark;

use super::{Shown, Steps, marks};

/// Whether `button` is the one whose hover shows what the payment would tap: Auto Pay.
pub(super) fn shows(button: ButtonId) -> bool {
    button == ButtonId::Auto
}

/// Draws the prompt's marks: what a click has picked to pay (`paying`), and what may be picked
/// (`eligible`). What Auto Pay would tap (`suggested`) is drawn as anything else that may be
/// picked until the pointer is on Auto Pay; then it alone is drawn, in its own mark, and the
/// outline of what else may be picked goes, as the web stage's `data-pay` rules in
/// `ui/src/stage/stage.css` have it.
pub(super) fn own(shown: Option<Shown>, was: Option<Shown>, steps: &mut Steps) {
    marks(shown, was, &look, steps);
}

/// The mark the table draws for `marked` while the payment is `shown`.
fn look(shown: Shown, marked: Marked) -> Option<Mark> {
    match (marked.mark, shown.suggesting) {
        (Mark::Suggested, false) => Some(Mark::Eligible),
        (Mark::Eligible, true) => None,
        (mark, _) => Some(mark),
    }
}

mod view;

prompt_feature!(Pay, view::view, own);

/// A mana payment's title already names the full cost, so show an amount next
/// to it only after some mana has been paid. Other costs always show the amount.
pub(super) fn shows_remaining(
    prompt: &beryl_protocol::prompt::PromptView,
    cost: &str,
    left: &str,
) -> bool {
    !prompt.is(beryl_protocol::prompt::Purpose::Mana) || left != cost
}
