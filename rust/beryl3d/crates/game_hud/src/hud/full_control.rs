//! Full control's stop sign on the desktop, left of the turn button while full control is on, as
//! the web interface's `.full-control` stands it among the turn controls (`ui/src/stage/Chrome.tsx`
//! and `ui/src/screens/styles/controls.css`): `right: 301px; top: 775px` in a 1600 by 900 stage,
//! moved left by the round pass-on button and its gap while the prompt has one. The phone shows
//! the sign in its middle band instead (`super::phase::middle`).

use beryl_protocol::prompt::ButtonId;
use glam::Vec2;

use crate::hud::HudIn;
use kit_base::tokens::{color, shadow, size, space};
use kit_frame::frame::Role;
use kit_ui::element::{Anchor, Element, Shown};
use kit_ui::widgets::icons;

/// The stop sign, while full control is on during a game on the desktop. Whether it is on is
/// read from the stops the game holds (`PlayerView::stops`), as the web's game screen reads
/// `view.stops`.
#[must_use]
pub fn full_control(input: &HudIn) -> Option<Element> {
    let on = input.view.is_some_and(|view| view.stops.full_control);
    if !on {
        return None;
    }
    // `.turn-controls:has(.pass-on-button) .full-control`: moved over for the round button.
    let pass_on = input.prompt.is_some_and(|prompt| {
        (prompt.buttons.iter()).any(|button_view| button_view.id == ButtonId::PassOn)
    });
    let dock = input.profile.slot("full_control")?.rect;
    let moved = if pass_on {
        size::BUTTON_HEIGHT + space::M
    } else {
        0.0
    };
    let side = f32::from(icons::FULL_CONTROL_SIDE);
    // An icon named by its tip, as the web's sign is an `svg` under a `Tip` that says "Full
    // control".
    let mut mark = Element::new(Role::Icon)
        .name("Full control")
        .width(side)
        .height(side)
        .color(color::GOLD)
        .drop_shadow(shadow::FULL_CONTROL.to_vec())
        .passes(true);
    mark.shows = Some(Shown::Icon(icons::FULL_CONTROL, icons::FULL_CONTROL_SIDE));
    // `.full-control`'s `top: 775px` against the turn button's top at 765 in a 900-point stage:
    // the sign is centred on the turn button.
    let top = dock.y + (dock.height - side) / 2.0;
    let corner = Vec2::new(dock.x + dock.width - moved, top);
    mark.anchor = Some(Anchor::Point(corner, Vec2::new(1.0, 0.0)));
    Some(mark)
}
