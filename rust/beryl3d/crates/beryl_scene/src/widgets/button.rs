//! Action buttons, with the looks of the web interface's `buttons.css`
//! (`ui/src/design/components/buttons.css`): a pill of one height and type under a 1.5-point
//! rim. The ordinary button is a near-black gradient under a pale rim, as the prompt bar shows
//! the session's buttons; the primary one is a deep amber gradient under a gold rim, the action a
//! prompt expects. Neither has a background color under its gradient, as on the web. A disabled one of either is flat stone with dim type, and one under the pointer is
//! a little brighter (the flat layer's `write` draws those states). A label too long for its
//! button shrinks to fit (`Style::fit`), as the web's `TurnButton` shrinks its label.

use super::icons;
use crate::element::{Align, Element, Justify, Shown, TextAlign};
use crate::frame::Role;
use crate::tokens::{color, gradient, ratio, shadow, size, type_style};

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
    element.style.box_shadow = shadow::SECONDARY.to_vec();
    element.style.background_image = vec![gradient::SECONDARY];
    element
        .background([0; 4])
        .border((color::SECONDARY_RIM, size::BORDER))
        .color(color::SECONDARY_INK)
}

/// The button a prompt expects to be pressed, showing `label`: deep amber under a gold rim,
/// with light type.
#[must_use]
pub fn primary(label: &str) -> Element {
    let mut element = button(label)
        .border((color::PRIMARY_RIM, size::BORDER))
        .color(color::PRIMARY_INK);
    element.primary = true;
    element.style.box_shadow = shadow::PRIMARY.to_vec();
    element.style.background_image = vec![gradient::PRIMARY];
    element
}

/// The round button that passes on to a later stop, named `name`, as the web's `PassOn`
/// (`ui/src/stage/PromptBar.tsx`) draws it: a frameless button `side` points across that holds a
/// face in the primary button's amber (`.pass-on-face.button-primary` in
/// `ui/src/screens/styles/controls.css`), with the fast-forward glyph in the primary button's
/// light ink under its shadow. The face is not a `.button`, so it stays amber when disabled.
#[must_use]
pub fn pass_on(name: &str, side: f32) -> Element {
    // `.pass-on-face > svg`: the glyph casts the shadow, and is nudged right because its ink
    // leans left of its box. The face centres it, so a left margin of twice the nudge moves it
    // right by the nudge.
    let glyph_size = icons::atlas_size(icons::PASS_ON);
    let mut glyph = Element::new(Role::Icon)
        .name("")
        .width(f32::from(glyph_size))
        .height(f32::from(glyph_size) * ratio::PASS_ON_GLYPH_ASPECT)
        .margin([0.0, 0.0, 0.0, 2.0 * size::PASS_ON_GLYPH_NUDGE])
        .color(color::PRIMARY_INK)
        .drop_shadow(shadow::PASS_ON.to_vec())
        .passes(true);
    glyph.shows = Some(Shown::Icon(icons::PASS_ON, glyph_size));
    let mut face = Element::new(Role::Region)
        .name("")
        .width(side)
        .height(side)
        .radius(side / 2.0)
        .background([0; 4])
        .border((color::PRIMARY_RIM, size::BORDER))
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .passes(true)
        .child(glyph);
    face.style.box_shadow = shadow::PRIMARY.to_vec();
    face.style.background_image = vec![gradient::PRIMARY];
    let mut element = Element::new(Role::Button)
        .name(name)
        .width(side)
        .height(side)
        .background([0; 4])
        .child(face);
    element.style.border = Some(([0; 4], 0.0));
    element
}
