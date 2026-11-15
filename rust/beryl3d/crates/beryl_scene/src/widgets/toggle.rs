//! A switch that is on or off, such as full control on the settings sheet.

use crate::element::Element;
use crate::frame::Role;

/// A switch named `name` that is `on` or off. Its state is not drawn until the widgets track
/// draws toggles.
#[must_use]
pub fn toggle(name: &str, on: bool) -> Element {
    let _ = on;
    Element::new(Role::Toggle).name(name)
}
