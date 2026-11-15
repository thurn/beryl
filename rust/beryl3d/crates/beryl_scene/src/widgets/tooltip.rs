//! A tooltip: words that explain the element the pointer rests on.

use crate::element::text;

/// The tooltip saying `words` about the element named `of`, named for that element.
#[must_use]
pub fn tooltip(words: &str, of: &str) -> crate::element::Element {
    text(words).name(&format!("tooltip {of}"))
}
