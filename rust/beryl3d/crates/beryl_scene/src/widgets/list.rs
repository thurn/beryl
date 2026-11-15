//! A list of rows, such as the game log or the card gallery's entries.

use crate::element::{Element, column};
use crate::frame::Role;

/// A list named `name` showing `rows` top to bottom.
#[must_use]
pub fn list(name: &str, rows: impl IntoIterator<Item = Element>) -> Element {
    let mut element = column().name(name).children(rows);
    element.role = Role::List;
    element
}
