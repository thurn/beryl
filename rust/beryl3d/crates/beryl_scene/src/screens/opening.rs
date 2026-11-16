//! The opening hand and the mulligan, shown on the game screen before the first turn: the web
//! interface's `Opening` (`ui/src/screens/Opening.tsx`) with the rules of `.opening` in
//! `ui/src/screens/styles/opening.css`. A dimmed table, whether the viewer is on the play, a
//! title, the viewer's hand in a row of large cards and, while cards are being put on the
//! bottom of the library, a row of slots under it for them. The open prompt's buttons stand in
//! the prompt bar, which `screens::game` adds after this, and the life totals are hidden, as the
//! web's overlay covers them.
//!
//! The cards are the hand's scene objects (`Shown::Object`): the table moves each from the hand
//! into its box and back (`table::shown`), so no card is drawn at the hand while the opening is
//! shown. A put-on-bottom pick clicks the cards in the row (`Click::On`); the session marks each
//! one picked as selected, and the row shows it in a slot.
//!
//! The coin is the `announce` notice's float (`hud::notices`), which the client draws over
//! every screen.
//!
//! Not built yet: the web's face-down hand while the viewer has declared a mulligan, and its
//! play or draw buttons in the middle of the window, which stand in the prompt bar here.

use beryl_protocol::PlayerView;
use beryl_protocol::prompt::{PromptView, Purpose};
use beryl_protocol::protocol::{Declaration, ObjectId, Side, Status, ZoneName};
use beryl_protocol::script::{Mark, Subject};
use glam::Vec2;

use super::ScreenIn;
use crate::data::space::flat::opening as metrics;
use crate::element::{Align, Anchor, Element, Shown, column, row, text};
use crate::frame::Role;
use crate::table::{CARD_HEIGHT, CARD_WIDTH};
use crate::text::TypeStyle;
use crate::text::fonts::Font;
use crate::tokens::color;

/// The starting hand size ([CR 103.5]): "Each player draws a number of cards equal to their
/// starting hand size, which is normally seven."
///
/// [CR 103.5]: https://yawgatog.com/resources/magic-rules/#R1035
const STARTING_HAND: usize = 7;

/// The title, `.opening-title`: 56 points of the serif face, bold.
const TITLE: TypeStyle = TypeStyle {
    font: Font::CrimsonPro,
    weight: 700,
    size: 56.0,
    line_height: 1.05,
};

/// Whether the viewer is on the play, `.opening-coin`: 22 points of the serif face.
const COIN: TypeStyle = TypeStyle {
    font: Font::CrimsonPro,
    weight: 600,
    size: 22.0,
    line_height: 1.2,
};

/// Whether the game is in its opening, as the web's `GameScreen` decides it
/// (`ui/src/screens/Game.tsx`): before the first turn, while the game is being decided or
/// played rather than free, over or stopped.
fn in_opening(view: &PlayerView) -> bool {
    view.turn.number == 0
        && !matches!(
            view.status,
            Status::Free | Status::Over { .. } | Status::Stopped
        )
}

/// Whether the game screen shows the opening.
#[must_use]
pub fn shown(input: &ScreenIn) -> bool {
    input.view.is_some_and(in_opening)
}

/// What the game screen shows in the opening, or none once it is over.
#[must_use]
pub fn opening(input: &ScreenIn) -> Option<Element> {
    let view = input.view.filter(|view| in_opening(view))?;
    let you = &view.players.you;
    let starting = view.turn.starting;
    let picking = input
        .prompt
        .filter(|prompt| prompt.is(Purpose::PutOnBottom));
    let hand: Vec<ObjectId> = (view.objects.iter())
        .filter(|object| object.zone == ZoneName::Hand && object.owner == Side::You)
        .map(|object| object.slot)
        .collect();
    let picked = |slot: ObjectId| picking.is_some_and(|prompt| selected(prompt, slot));
    let bottom: Vec<ObjectId> = hand.iter().copied().filter(|&slot| picked(slot)).collect();
    // One card per mulligan goes on the bottom, as the web's `toBottom` counts them.
    let to_bottom = if picking.is_some() {
        hand.len()
            .saturating_sub(STARTING_HAND - usize::from(you.mulligans))
            .max(1)
    } else {
        0
    };
    let title = title(view, picking.is_some());
    let mut coin = row().name("coin").height(metrics::COIN_HEIGHT);
    coin.style.margin = [0.0, 0.0, metrics::COIN_GAP, 0.0];
    if let Some(starting) = starting {
        let words = if starting == Side::You {
            "On the play"
        } else {
            "On the draw"
        };
        coin = coin
            .align_items(Align::Center)
            .child(text(words).style(COIN).color(color::GOLD).passes(true));
    }
    let mut heading = text(&title).style(TITLE).color(color::TEXT).passes(true);
    heading.style.margin = [0.0, 0.0, metrics::TITLE_GAP, 0.0];
    let cards = (hand.iter())
        .filter(|&&slot| !picked(slot))
        .filter(|_| starting.is_some())
        .map(|&slot| card_box(view, slot, metrics::CARD));
    let hand_row = row().name("hand").gap(metrics::CARD_GAP).children(cards);
    let slots = to_bottom.max(bottom.len());
    let slots_row = (slots > 0).then(|| {
        let boxes = (0..slots).map(|at| match bottom.get(at) {
            Some(&slot) => card_box(view, slot, metrics::SLOT),
            None => empty_slot(at),
        });
        let mut slots_row = row()
            .name("Bottom of your library")
            .gap(metrics::SLOT_GAP)
            .children(boxes);
        slots_row.style.margin = [metrics::SLOTS_GAP, 0.0, 0.0, 0.0];
        slots_row
    });
    let mut screen = column()
        .name("opening")
        .passes(true)
        .align_items(Align::Center)
        .child(coin)
        .child(heading)
        .child(hand_row)
        .children(slots_row);
    // The flat layer's root puts a screen in its bottom right corner, so the group is anchored
    // below the window's top middle instead, as the web pads `.opening` down from the top.
    let top = Vec2::new(input.window.size.width / 2.0, metrics::TOP);
    screen.anchor = Some(Anchor::Point(top, Vec2::new(0.5, 0.0)));
    Some(
        row()
            .name("opening screen")
            .passes(true)
            .child(scrim(input))
            .child(screen),
    )
}

/// The title, as the web's `Opening` words it.
fn title(view: &PlayerView, picking: bool) -> String {
    let you = &view.players.you;
    match view.turn.starting {
        None if view.turn.flip_winner == Some(Side::You) => "You won the flip".to_owned(),
        None => "Opponent won the flip".to_owned(),
        Some(_) if picking => "Put on the bottom".to_owned(),
        Some(_) if you.mulligan == Declaration::WillMulligan => "Mulligan".to_owned(),
        Some(_) if you.mulligans == 0 => "Opening hand".to_owned(),
        Some(_) => format!(
            "Mulligan to {}",
            STARTING_HAND.saturating_sub(usize::from(you.mulligans))
        ),
    }
}

/// Whether the session marks the card in `slot` picked in `prompt`.
fn selected(prompt: &PromptView, slot: ObjectId) -> bool {
    (prompt.marks.iter())
        .any(|marked| marked.mark == Mark::Selected && marked.on == Subject::Object { slot })
}

/// The box `width` points wide that shows the scene object in `slot`, named for its card and
/// slot as a locator names a card ("Island #8"). The box takes no press: the card drawn in it
/// is the table's, and a click on it clicks the open prompt on the card (`Client::frame`).
fn card_box(view: &PlayerView, slot: ObjectId, width: f32) -> Element {
    let name = (view.objects.iter())
        .find(|object| object.slot == slot)
        .and_then(|object| object.name.as_deref())
        .unwrap_or("card");
    let mut element = Element::new(Role::Region)
        .name(&format!("{name} #{}", slot.0))
        .width(width)
        .height(width * CARD_HEIGHT / CARD_WIDTH);
    element.shows = Some(Shown::Object(Subject::Object { slot }));
    element
}

/// An empty slot for a card still to be put on the bottom, `.opening-slot`.
fn empty_slot(at: usize) -> Element {
    Element::new(Role::Region)
        .name(&format!("slot {}", at + 1))
        .width(metrics::SLOT)
        .height(metrics::SLOT * CARD_HEIGHT / CARD_WIDTH)
        .background(color::BACKDROP)
        .passes(true)
}

/// The dimmed table, `.overlay-backdrop`: the whole window in the backdrop's color, which the
/// pointer goes through.
fn scrim(input: &ScreenIn) -> Element {
    let mut scrim = row()
        .name("scrim")
        .width(input.window.size.width)
        .height(input.window.size.height)
        .background(color::BACKDROP)
        .passes(true);
    scrim.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
    scrim
}
