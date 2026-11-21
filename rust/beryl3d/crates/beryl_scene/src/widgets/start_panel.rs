//! The panel and controls shared by the start and lobby screens.

use crate::element::{Align, Border, Element, Justify, row, text};
use kit_base::tokens::{color, ratio, shadow, size, space, type_style};
use kit_frame::frame::Role;

const CHECKBOX: f32 = size::CHECKBOX;

/// The panel's width and padding (`.start-panel`).
pub(crate) const PANEL_WIDTH: f32 = size::START_PANEL_WIDTH;
pub(crate) const PANEL_PADDING: [f32; 4] = [
    size::START_PANEL_PADDING_TOP,
    size::START_PANEL_PADDING_SIDE,
    size::START_PANEL_PADDING_BOTTOM,
    size::START_PANEL_PADDING_SIDE,
];
pub(crate) const PANEL_GAP: f32 = size::START_PANEL_GAP;
const PANEL_RADIUS: f32 = size::PANEL_RADIUS;

/// A checkbox labelled `label`, ticked if `on`.
pub(crate) fn checkbox(label: &str, on: bool) -> Element {
    let check = check_square(CHECKBOX, on);
    Element::new(Role::Toggle)
        .name(label)
        .selected(on)
        .gap(space::M)
        .align_items(Align::Center)
        .child(check)
        .child(
            text(label)
                .style(type_style::START_LABEL)
                .color(color::TEXT_DIM),
        )
}

/// `panel` in the look of the web's `.start-panel`, which the start and lobby screens share: a
/// dark face, a bronze border with a lighter top edge, rounded corners, a dark ring inside the
/// border and a shadow under it.
pub(crate) fn panel_look(panel: Element) -> Element {
    panel
        .background(color::START_PANEL)
        .border(
            Border::uniform(color::START_PANEL_BORDER, size::BORDER)
                .side_color(0, color::START_PANEL_BORDER_TOP),
        )
        .radius(PANEL_RADIUS)
        .box_shadow(shadow::START_PANEL.to_vec())
}

/// The square of a checkbox `side` points wide, with a gold border, ticked if `on`.
pub(crate) fn check_square(side: f32, on: bool) -> Element {
    row()
        .name("checkbox")
        .width(side)
        .height(side)
        .radius(size::START_CHECKBOX_RADIUS)
        .border((color::GOLD, size::BORDER))
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        // No bundled font has a check mark, so a ticked box is filled with a gold square, as the
        // chosen opponent's radio button is filled with a dot.
        .children(on.then(|| {
            row()
                .name("tick")
                .width(side / 2.0)
                .height(side / 2.0)
                .radius(size::START_TICK_RADIUS)
                .background(color::GOLD)
        }))
}

/// `button` at the web's small size (`button-small`): shorter, with smaller type.
pub(crate) fn small(mut button: Element) -> Element {
    button.style.height = Some(size::BUTTON_HEIGHT * ratio::SMALL_SCALE);
    button.style.text.size = type_style::BUTTON.size * ratio::SMALL_SCALE;
    button.style.min_width = None;
    button
}
