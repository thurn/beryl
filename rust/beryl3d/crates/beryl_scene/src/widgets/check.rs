//! The mark of a checked box or menu row: a filled gold square. The bundled fonts have no
//! check mark glyph (U+2713), so text drawn with it shows nothing; this draws the mark as a
//! box instead, as the shared checkbox does.

use crate::element::{Element, row};
use crate::tokens::{color, ratio};

/// A gold square `side` logical points on a side, the mark of a checked box or row.
#[must_use]
pub fn check_mark(side: f32) -> Element {
    row()
        .name("mark")
        .width(side)
        .height(side)
        .radius(side * ratio::QUARTER)
        .background(color::GOLD)
}
