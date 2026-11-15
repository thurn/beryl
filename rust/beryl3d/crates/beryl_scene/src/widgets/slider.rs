//! A slider that sets a level, such as the music's volume.

use crate::element::Element;
use crate::frame::Role;

/// A slider named `name` set at `value`, from 0 to 1. Its value is not drawn until the
/// widgets track draws sliders.
#[must_use]
pub fn slider(name: &str, value: f32) -> Element {
    let _ = value;
    Element::new(Role::Toggle).name(name)
}
