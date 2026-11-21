//! The chrome of the Revealed Cards pile (`table::revealed`), as the web interface's
//! `RevealedCards` (`ui/src/stage/Revealed.tsx`) draws it: a pill over the pile that says
//! "Revealed Cards", which opens a viewer of the pile's faces, beside a cross that dismisses the
//! pile. The pile's faces appear while a script plays, so the pill is a float the client shows
//! whenever the pile holds a face (`floats::REVEALED`), laid out once ahead of time, and not a
//! piece of the game screen.

use crate::hud::GamePress;
use crate::inspect::Viewed;
use beryl_protocol::protocol::{ObjectId, Side};
use glam::Vec2;

use crate::table::phone::strip;
use crate::table::{CARD_HEIGHT, CARD_WIDTH, Window};
use kit_base::data::space::hud::phone::REVEALED_WIDTH;
use kit_base::data::space::layout::phone::bands::OPPONENT_STRIP;
use kit_base::data::space::revealed as data;
use kit_base::tokens::{color, size, type_style};
use kit_frame::frame::Role;
use kit_ui::element::{Align, Element, Shown, row};
use kit_ui::input::{FaceId, Size};
use kit_ui::widgets::{button, icons};

/// What the pile and its viewer are called: Arena's words, as the web's `REVEALED`.
pub const REVEALED: &str = "Revealed Cards";

/// A face in the Revealed Cards pile, the first revealed first, as the viewer of the pile lists
/// it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RevealedFace {
    /// The slot of the card it is the face of.
    pub slot: ObjectId,
    /// The card's name.
    pub name: String,
    /// Its face, once the match's card records have arrived.
    pub face: Option<FaceId>,
}

/// The pill and its cross: a small secondary button that opens the pile's viewer, and an icon
/// that dismisses the pile.
#[must_use]
pub fn revealed() -> Element {
    let mut pill = button(REVEALED)
        .on_press(GamePress::View(Viewed::Revealed))
        .height(size::PILL_HEIGHT)
        .min_width(0.0)
        .style(type_style::PILL);
    pill.style.padding = [0.0, size::PILL_PADDING, 0.0, size::PILL_PADDING];
    let mut cross = Element::new(Role::Button)
        .name("Dismiss")
        .on_press(GamePress::DismissRevealed)
        .width(size::PILL_CROSS)
        .height(size::PILL_CROSS)
        .background(color::ICON_BUTTON)
        .radius(size::PILL_CROSS / 2.0);
    cross.shows = Some(Shown::Icon(icons::CLOSE, cross_icon()));
    row()
        .name(REVEALED)
        .gap(size::PILL_GAP)
        .align_items(Align::Center)
        .child(pill)
        .child(cross)
}

/// The pile's button on the phone, which shows no pill (docs/ui-conventions.md, "The phone"):
/// a clear button as large as a finger over the opponent's strip beside their graveyard, where
/// the pile's faces lie at a pile's size, which opens the pile's viewer as a press on a pile
/// does (`.phone-pile-button` in `ui/src/mobile/phone.css`).
#[must_use]
pub fn phone_pile() -> Element {
    Element::new(Role::Button)
        .name(REVEALED)
        .on_press(GamePress::View(Viewed::Revealed))
        .width(REVEALED_WIDTH)
        .height(OPPONENT_STRIP)
        .background([0; 4])
        .border(([0; 4], 0.0))
}

/// Where the phone's button over the pile stands in a phone `window`, its top left corner in
/// logical points: centred on the opponent's third pile place, after their library and
/// graveyard, which the phone leaves free since it rests exiled cards under the graveyard.
#[must_use]
pub fn phone_pile_corner(window: Window) -> Vec2 {
    let strip = strip(window, Side::Opponent);
    let x = window.safe_area.left + kit_base::data::space::layout::phone::piles::opponent::X.z;
    Vec2::new(x - REVEALED_WIDTH / 2.0, strip.top)
}

/// Where the pill's top left corner stands in a window of `window`'s size, in logical points:
/// a little left of and above the pile's first face, as `RevealedCards` sets the pill's `left`
/// and `top` from the pile's space.
#[must_use]
pub fn pill_corner(window: Size) -> Vec2 {
    // The first face's centre, from normalized device coordinates, and its half size.
    let centre = Vec2::new(
        f32::midpoint(data::AT[0], 1.0) * window.width,
        (1.0 - data::AT[1]) / 2.0 * window.height,
    );
    let height = data::HEIGHT * window.height;
    let half = Vec2::new(height * CARD_WIDTH / CARD_HEIGHT, height) / 2.0;
    centre - half + Vec2::new(data::PILL_X, data::PILL_Y)
}

/// The side of the cross's glyph, in whole points: the size the icon atlas holds the cross at,
/// an icon button's.
fn cross_icon() -> u16 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few points"
    )]
    let side = size::ICON as u16;
    side
}
