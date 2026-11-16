//! Action buttons, with the looks of the web interface's `buttons.css`
//! (`ui/src/design/components/buttons.css`): a pill of one height and type under a 1.5-point
//! rim. The ordinary button is near black under a pale rim, as the prompt bar shows the
//! session's buttons; the primary one is deep amber under a gold rim, the action a prompt
//! expects. A disabled one of either is flat stone with dim type, and one under the pointer is
//! a little brighter (the flat layer's `write` draws those states). A label too long for its
//! button shrinks to fit (`Style::fit`), as the web's `TurnButton` shrinks its label.

use crate::element::{Element, TextAlign};
use crate::frame::Role;
use crate::tokens::{color, size, type_style};

/// A button showing `label`, named by it: as tall as every action button and at least 8 ems
/// wide, with its label centred.
#[must_use]
pub fn button(label: &str) -> Element {
    let mut element = Element::new(Role::Button).name(label);
    element.text = Some(label.to_owned());
    element.style.text = type_style::BUTTON;
    element.style.text_align = TextAlign::Center;
    element.style.height = Some(size::BUTTON_HEIGHT);
    element.style.min_width = Some(size::BUTTON_MIN_WIDTH);
    element.style.padding = [0.0, size::BUTTON_PADDING, 0.0, size::BUTTON_PADDING];
    element.style.fit = true;
    element
        .background(color::SECONDARY)
        .border((color::SECONDARY_RIM, size::BORDER))
        .color(color::SECONDARY_INK)
}

/// The button a prompt expects to be pressed, showing `label`: deep amber under a gold rim,
/// with light type.
#[must_use]
pub fn primary(label: &str) -> Element {
    button(label)
        .background(color::PRIMARY)
        .border((color::PRIMARY_RIM, size::BORDER))
        .color(color::PRIMARY_INK)
}
