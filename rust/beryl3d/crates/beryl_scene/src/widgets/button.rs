//! Action buttons: the ordinary button, as the prompt bar shows the session's buttons, and the
//! primary one, which stands out as the action a prompt expects.

use crate::element::{Element, TextAlign};
use crate::frame::Role;
use crate::tokens::{size, type_style};

/// A button showing `label`, named by it: as tall as every action button, with its label
/// centred.
#[must_use]
pub fn button(label: &str) -> Element {
    let mut element = Element::new(Role::Button).name(label);
    element.text = Some(label.to_owned());
    element.style.text = type_style::BUTTON;
    element.style.text_align = TextAlign::Center;
    element.style.height = Some(size::BUTTON_HEIGHT);
    element.style.padding = [0.0, size::BUTTON_PADDING, 0.0, size::BUTTON_PADDING];
    element
}

/// The button a prompt expects to be pressed, showing `label`. Drawn as `button` until the
/// widgets track gives it a look of its own.
#[must_use]
pub fn primary(label: &str) -> Element {
    button(label)
}
