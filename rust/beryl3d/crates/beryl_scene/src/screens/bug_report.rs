//! The bug report sheet: for now its description field alone, until the screens track builds
//! the rest of it.

use super::ScreenIn;
use crate::element::{Element, column};
use crate::widgets::field;

/// The name of the field the player describes the bug in.
pub const DESCRIPTION: &str = "description";

/// How wide the description field is, in logical points.
const DESCRIPTION_WIDTH: f32 = 480.0;

/// The least height of the description field: the web interface's `.bug-text` rule's
/// `min-height` (`ui/src/screens/styles/bug-report.css`).
const DESCRIPTION_HEIGHT: f32 = 110.0;

/// What the flat layer shows for it.
#[must_use]
pub fn bug_report(input: &ScreenIn) -> Element {
    let mut state = input.fields.get(DESCRIPTION);
    "What went wrong?".clone_into(&mut state.placeholder);
    let mut description = field(DESCRIPTION, &state);
    description.style.width = Some(DESCRIPTION_WIDTH);
    description.style.min_height = Some(DESCRIPTION_HEIGHT);
    column().name("bug report").child(description)
}
