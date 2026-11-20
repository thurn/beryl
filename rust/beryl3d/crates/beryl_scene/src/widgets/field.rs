//! A text field, such as the bug report's description. While it has the focus the layer edits
//! it in place (`layout::Edit`): typing, the arrow keys with Shift to select, Home and End, a
//! double click to select a word, a drag to select, and Cmd+A, Cmd+C, Cmd+X and Cmd+V. Each
//! change comes back to the client as `Press::Field`, which keeps it in `Fields`.

use std::collections::BTreeMap;
use std::ops::Range;

use crate::element::{Border, Element};
use crate::frame::Role;
use crate::tokens::{color, size, type_style};

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

/// The state of every text field the screens show, by the field's name. The client keeps it
/// and hands it to the screen functions (`ScreenIn::fields`), so that what a person typed
/// stays when the screen is built again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Fields(BTreeMap<String, FieldState>);

impl Fields {
    /// No field's state: every field empty.
    #[must_use]
    pub const fn new() -> Self {
        Self(BTreeMap::new())
    }

    /// The state of the field named `name`: empty until something is typed in it.
    #[must_use]
    pub fn get(&self, name: &str) -> FieldState {
        self.0.get(name).cloned().unwrap_or_default()
    }

    /// Keeps `state` as the state of the field named `name`.
    pub fn set(&mut self, name: &str, state: FieldState) {
        self.0.insert(name.to_owned(), state);
    }
}

/// A text field named `name` holding `state`. It takes the focus when pressed, and shows its
/// placeholder, faint, while it is empty. It looks like the web interface's bug report text
/// (`.bug-text` in `ui/src/screens/styles/bug-report.css`): sunk, with a metal border and
/// corners of 6 points, padded 10 points above and below and 12 at the sides.
#[must_use]
pub fn field(name: &str, state: &FieldState) -> Element {
    let mut element = Element::new(Role::Field).name(name);
    element.focus = true;
    let empty = state.text.is_empty();
    let words = if empty {
        &state.placeholder
    } else {
        &state.text
    };
    if !words.is_empty() {
        element.text = Some(words.clone());
    }
    if empty {
        element.style.color = Some(color::TEXT_FAINT);
    }
    element.style.text = type_style::FIELD;
    let (y, x) = (size::FIELD_PADDING_Y, size::FIELD_PADDING_X);
    element.style.padding = [y, x, y, x];
    element.style.min_height = Some(type_style::FIELD.size * type_style::FIELD.line_height);
    element.style.background = Some(color::SURFACE_SUNK);
    element.style.border = Some(Border::uniform(color::METAL, size::FIELD_BORDER));
    element.style.radius = size::FIELD_RADIUS;
    element.field = Some(Box::new(state.clone()));
    element
}
