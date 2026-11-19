//! A button that shows an icon instead of words, such as the settings gear, with the look of
//! the web interface's `icon-button.css` (`ui/src/design/components/icon-button.css`): a round
//! face of dark glass 44 points across with a faint gold rim, its 22-point glyph in the dim text
//! color. Under the pointer the rim and the glyph brighten, and held down the face darkens (the
//! flat layer's `write` draws those states).

use super::{IconId, icons};
use crate::element::{Element, Shown};
use crate::frame::Role;
use crate::tokens::{color, size};

/// A button showing the icon `icon`, named `name` because it has no words.
#[must_use]
pub fn icon_button(icon: IconId, name: &str) -> Element {
    round(Role::Button, icon, name)
}

/// A round icon control of `role`: the face, rim and glyph every icon button has.
pub(super) fn round(role: Role, icon: IconId, name: &str) -> Element {
    let mut element = Element::new(role)
        .name(name)
        .width(size::ICON_BUTTON)
        .height(size::ICON_BUTTON)
        .background(color::ICON_BUTTON)
        .border((color::ICON_BUTTON_RIM, size::ICON_BUTTON_RIM))
        .radius(size::ICON_BUTTON / 2.0);
    // At the one size the atlas holds the icon: 22 points for most, as `size::ICON` is, and 28
    // for full control's mark, whose artwork is drawn larger.
    element.shows = Some(Shown::Icon(icon, icons::atlas_size(icon)));
    element
}
