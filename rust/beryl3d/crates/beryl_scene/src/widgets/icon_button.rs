//! A button that shows an icon instead of words, such as the settings gear.

use super::IconId;
use crate::element::Element;
use crate::frame::Role;

/// A button showing the icon `icon`, named `name` because it has no words. The icon is not
/// drawn until the widgets track draws icons.
#[must_use]
pub fn icon_button(icon: IconId, name: &str) -> Element {
    let _ = icon;
    Element::new(Role::Button).name(name)
}
