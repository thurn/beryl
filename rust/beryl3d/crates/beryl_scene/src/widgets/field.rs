//! A text field, such as the bug report's description.

use std::ops::Range;

use crate::element::Element;
use crate::frame::Role;

/// What a text field holds and where its caret is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldState {
    pub text: String,
    /// The caret's byte offset in `text`.
    pub caret: u32,
    /// The selected bytes of `text`, if any.
    pub selection: Option<Range<u32>>,
    /// The words shown while `text` is empty.
    pub placeholder: String,
}

/// A text field named `name` holding `state`. It takes keys while it has the focus. Its text,
/// caret and selection are not drawn until the widgets track draws fields.
#[must_use]
pub fn field(name: &str, state: &FieldState) -> Element {
    let _ = state;
    let mut element = Element::new(Role::Field).name(name);
    element.focus = true;
    element
}
