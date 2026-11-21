//! The prompt shape `Pay`: a cost paid with permanents, such as mana, a crew cost or a
//! waterbend cost (UI Conventions, "Paying with permanents"). The prompt bar says how much is
//! still to pay, and the table marks what Auto Pay would tap only while the pointer is on Auto
//! Pay: until then those permanents are outlined as anything else that may be picked. A click
//! on a permanent where it lies picks it or takes it back, through the table's pointer.

use beryl_protocol::prompt::{ButtonId, Marked, PromptView, Purpose, ShapeBody};
use beryl_protocol::script::Mark;

use super::bar::bar;
use super::{Shown, Steps, marks};
use crate::hud::HudIn;
use kit_ui::element::Element;

/// The prompt bar, with how much of the cost is still to pay after what it asks, as the web's
/// `Asks` (`ui/src/stage/PromptBar.tsx`) writes it. A mana payment's title already names the
/// whole cost ("Pay {1}{W}."), so before anything is picked it shows the amount once. A crew or
/// waterbend cost keeps its amount beside the title.
#[expect(
    clippy::unnecessary_wraps,
    reason = "every shape returns an `Option`: a built shape may show nothing in the flat layer"
)]
pub(super) fn screen(prompt: &PromptView, input: &HudIn) -> Option<Element> {
    let left = match &prompt.body {
        ShapeBody::Pay { cost, left } if !(prompt.is(Purpose::Mana) && left == cost) => {
            Some(left.as_str())
        }
        _ => None,
    };
    Some(bar(prompt, input, left, None))
}

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
