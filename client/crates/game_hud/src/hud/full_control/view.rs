//! Full control's stop sign on the desktop, left of the turn button while full control is on, as
//! the web interface's `.full-control` stands it among the turn controls (`ui/src/stage/Chrome.tsx`
//! and `ui/src/screens/styles/controls.css`): `right: 301px; top: 775px` in a 1600 by 900 stage,
//! moved left by the round pass-on button and its gap while the prompt has one. The phone shows
//! the sign in its middle band instead (`super::phase::middle`).

use glam::Vec2;

use kit_base::geometry::Rect;
use kit_base::tokens::{color, shadow, size, space};
use kit_frame::frame::Role;
use kit_ui::element::{Anchor, Element, Shown};
use kit_ui::widgets::icons;

/// The dock's sign reads the session setting and makes room for Pass On.
pub(super) fn dock(dock: Rect, pass_on: bool) -> Element {
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
    mark
}

/// The sign at the end of the middle band across from the phase icon: the right end, or the
/// left end on a table whose players share their piles (`shared`), where the icon stands at
/// the right end.
pub(super) fn middle_band(slot: Rect, shared: bool) -> Element {
    let side = f32::from(icons::PHASE_SIDE);
    let line = slot.y + slot.height / 2.0;
    let mut mark = Element::new(Role::Icon)
        .name("full control")
        .width(side)
        .height(side)
        .color(color::GOLD)
        .drop_shadow(shadow::FULL_CONTROL.to_vec())
        .passes(true);
    mark.shows = Some(Shown::Icon(
        icons::FULL_CONTROL,
        icons::atlas_size(icons::FULL_CONTROL),
    ));
    mark.anchor = Some(if shared {
        Anchor::Point(Vec2::new(slot.x, line), Vec2::new(0.0, 0.5))
    } else {
        Anchor::Point(Vec2::new(slot.x + slot.width, line), Vec2::new(1.0, 0.5))
    });
    mark
}
