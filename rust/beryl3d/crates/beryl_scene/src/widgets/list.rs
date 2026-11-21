//! A list of rows, such as the game log or the card gallery's entries: a column that scrolls
//! when its rows pass its box. While it, or one of its rows, has the focus, the up and down
//! keys highlight the next or the previous row and Return presses the highlighted one
//! (`Layer::navigate`). A row scrolled out of the list's box takes no presses.

use crate::element::{Element, Overflow, column};
use kit_frame::frame::Role;

/// A list named `name` showing `rows` top to bottom. Each row that has a press, or takes the
/// focus, is one the keys move through.
#[must_use]
pub fn list(name: &str, rows: impl IntoIterator<Item = Element>) -> Element {
    let mut element = column()
        .name(name)
        .overflow(Overflow::Scroll)
        .children(rows);
    element.role = Role::List;
    element.focus = true;
    element
}
