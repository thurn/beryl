//! The opening hand and the mulligan, shown on the game screen before the first turn: the web
//! interface's `Opening` (`ui/src/screens/Opening.tsx`) with the rules of `.opening` in
//! `ui/src/screens/styles/opening.css`. A dimmed table, whether the viewer is on the play, a
//! title, the viewer's hand in a row of large cards and, while cards are being put on the
//! bottom of the library, a row of slots under it for them. The open prompt's buttons stand in
//! the prompt bar, which `screens::game` adds after this so that the backdrop is drawn under
//! them, and the life totals are hidden, as the web's overlay covers them.
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
use crate::data::space::flat::phone::opening as phone;
use crate::data::space::flat::phone::{ACTION_GAP, ACTION_HEIGHT, GUTTER};
use crate::element::{Align, Anchor, Element, Justify, Press, Shown, column, row, text};
use crate::frame::Role;
use crate::table::{CARD_HEIGHT, CARD_WIDTH, Window};
use crate::text::TypeStyle;
use crate::tokens::{color, type_style};

/// The starting hand size ([CR 103.5]): "Each player draws a number of cards equal to their
/// starting hand size, which is normally seven."
///
/// [CR 103.5]: https://yawgatog.com/resources/magic-rules/#R1035
const STARTING_HAND: usize = 7;

/// The title, `.opening-title`: 56 points of the serif face, bold.
const TITLE: TypeStyle = type_style::OPENING_TITLE;

/// Whether the viewer is on the play, `.opening-coin`: 22 points of the serif face.
const COIN: TypeStyle = type_style::OPENING_COIN;

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
    let actions = if picking.is_some() {
        1
    } else {
        input.prompt.map_or(0, |prompt| prompt.buttons.len())
    };
    let shown: Vec<ObjectId> = (hand.iter().copied())
        .filter(|&slot| !picked(slot))
        .filter(|_| starting.is_some())
        .collect();
    Some(present(
        &Presentation {
            view,
            starting,
            title: &title,
            hand: &shown,
            rows: hand.len(),
            bottom: &bottom,
            slots: to_bottom.max(bottom.len()),
            actions,
            press: None,
        },
        input,
    ))
}

/// What an opening shows: whether the viewer is on the play (none while that is still being
/// chosen), the title, the cards still in the hand row and those on their way to the bottom,
/// how many cards the hand holds in all (which sizes the phone's rows), how many slots the
/// bottom row has, how many buttons the dock holds, and what a press on a card does, if
/// anything. The engine's game and the manual table each fill it from their own prompt.
pub(crate) struct Presentation<'a> {
    pub view: &'a PlayerView,
    pub starting: Option<Side>,
    pub title: &'a str,
    pub hand: &'a [ObjectId],
    pub rows: usize,
    pub bottom: &'a [ObjectId],
    pub slots: usize,
    pub actions: usize,
    pub press: Option<fn(ObjectId) -> Press>,
}

/// The opening `shown` describes, over the dimmed table: the coin, the title, the hand and the
/// bottom slots, as the web's `OpeningPresentation` lays them out.
pub(crate) fn present(shown: &Presentation, input: &ScreenIn) -> Element {
    let Presentation {
        view,
        starting,
        slots,
        ..
    } = *shown;
    let sizes = if super::phone(input) {
        // Room for both rows while cards move between the hand and the slots, as the web
        // keeps it, and for the prompt's buttons in the dock under them.
        let rows = shown.rows.div_ceil(phone::PER_ROW);
        Sizes::phone(input.window, rows, slots, shown.actions)
    } else {
        Sizes::DESKTOP
    };
    let card = |slot: ObjectId, width: f32| {
        let card = card_box(view, slot, width);
        match shown.press {
            Some(press) => card.on_press(press(slot)),
            None => card,
        }
    };
    let mut coin = row().name("coin").height(sizes.coin_height);
    coin.style.margin = [0.0, 0.0, sizes.coin_gap, 0.0];
    if let Some(starting) = starting {
        let words = if starting == Side::You {
            "On the play"
        } else {
            "On the draw"
        };
        coin = coin.align_items(sizes.coin_align).child(
            text(words)
                .style(sizes.coin)
                .color(color::GOLD)
                .passes(true),
        );
    }
    let mut heading = text(shown.title)
        .style(sizes.title)
        .color(color::TEXT)
        .passes(true);
    heading.style.margin = [0.0, 0.0, sizes.title_gap, 0.0];
    let cards = (shown.hand.iter()).map(|&slot| card(slot, sizes.card));
    let mut hand_row = row().name("hand").gap(sizes.card_gap).children(cards);
    if let Some(width) = sizes.hand_width {
        // The phone's hand wraps in rows of four (`.phone-opening .opening-hand`).
        hand_row = hand_row
            .width(width)
            .wrap(true)
            .justify_content(Justify::Center);
    }
    let slots_row = (slots > 0).then(|| {
        let boxes = (0..slots).map(|at| match shown.bottom.get(at) {
            Some(&slot) => card(slot, sizes.slot),
            None => empty_slot(at, sizes.slot),
        });
        let mut slots_row = row()
            .name("Bottom of your library")
            .gap(sizes.slot_gap)
            .children(boxes);
        slots_row.style.margin = [sizes.slots_gap, 0.0, 0.0, 0.0];
        slots_row
    });
    // The pointer goes through to the table's cards, unless the cards here take presses of
    // their own, which they would inherit the passing from.
    let passes = shown.press.is_none();
    let mut screen = column()
        .name("opening")
        .passes(passes)
        .align_items(Align::Center)
        .child(coin)
        .child(heading)
        .child(hand_row)
        .children(slots_row);
    // The flat layer's root puts a screen in its bottom right corner, so the group is anchored
    // below the window's top middle instead, as the web pads `.opening` down from the top.
    let top = Vec2::new(input.window.size.width / 2.0, sizes.top);
    screen.anchor = Some(Anchor::Point(top, Vec2::new(0.5, 0.0)));
    row()
        .name("opening screen")
        .passes(passes)
        .child(super::backdrop(input))
        .child(screen)
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
fn empty_slot(at: usize, width: f32) -> Element {
    Element::new(Role::Region)
        .name(&format!("slot {}", at + 1))
        .width(width)
        .height(width * CARD_HEIGHT / CARD_WIDTH)
        .background(color::BACKDROP)
        .passes(true)
}

/// How large the opening's pieces are and where it stands, on one layout.
struct Sizes {
    /// How far the group's top stands below the window's top.
    top: f32,
    coin_height: f32,
    coin_gap: f32,
    /// Where the words of the coin stand in its row.
    coin_align: Align,
    coin: TypeStyle,
    title: TypeStyle,
    title_gap: f32,
    /// The width of each card of the hand, and of each card on its way to the bottom.
    card: f32,
    slot: f32,
    card_gap: f32,
    slot_gap: f32,
    slots_gap: f32,
    /// The width the hand wraps at, on the phone.
    hand_width: Option<f32>,
}

impl Sizes {
    /// The desktop's: the hand in one row of large cards (`.opening`).
    const DESKTOP: Self = Self {
        top: metrics::TOP,
        coin_height: metrics::COIN_HEIGHT,
        coin_gap: metrics::COIN_GAP,
        coin_align: Align::Center,
        coin: COIN,
        title: TITLE,
        title_gap: metrics::TITLE_GAP,
        card: metrics::CARD,
        slot: metrics::SLOT,
        card_gap: metrics::CARD_GAP,
        slot_gap: metrics::SLOT_GAP,
        slots_gap: metrics::SLOTS_GAP,
        hand_width: None,
    };

    /// The phone's in `window` (`.phone-opening`, `phoneCardWidth` in
    /// `ui/src/screens/Opening.tsx`): the hand in rows of four cards, as large as fit between
    /// the coin and the dock with `rows` rows of the hand, `slots` slots for cards going to the
    /// bottom and `actions` buttons in the dock.
    fn phone(window: Window, rows: usize, slots: usize, actions: usize) -> Self {
        let safe = window.safe_area;
        let narrow = window.size.width < phone::NARROW_BELOW;
        let coin = if narrow {
            phone::COIN_NARROW
        } else {
            phone::COIN
        };
        let per_row = phone::PER_ROW as f32;
        let height = window.size.height - safe.top - safe.bottom;
        let header = phone::TOP
            + coin / 2.0
            + phone::COIN_LINE
            + phone::COIN_GAP
            + phone::TITLE
            + phone::TITLE_GAP;
        let footer = GUTTER
            + if actions > 0 {
                actions as f32 * ACTION_HEIGHT + (actions - 1) as f32 * ACTION_GAP
            } else {
                phone::NO_ACTIONS
            };
        let gaps = rows.saturating_sub(1) as f32 * phone::GAP
            + if slots > 0 { phone::SLOTS_GAP } else { 0.0 };
        let card_rows = rows as f32 + if slots > 0 { phone::SLOT_SCALE } else { 0.0 };
        let most = if narrow {
            phone::CARD_MOST_NARROW
        } else {
            phone::CARD_MOST
        };
        let across =
            (window.size.width - safe.left - safe.right - (per_row - 1.0) * phone::GAP) / per_row;
        let down =
            (height - header - footer - gaps) * CARD_WIDTH / CARD_HEIGHT / card_rows.max(1.0);
        let card = most.min(across).min(down).max(0.0).floor();
        let slot = if slots > 0 {
            let room = window.size.width - safe.left - safe.right - phone::SLOT_EDGE;
            (phone::SLOT_SCALE * card).min((room - (slots - 1) as f32 * phone::GAP) / slots as f32)
        } else {
            card
        };
        Self {
            top: safe.top + phone::TOP - coin / 2.0,
            coin_height: coin + phone::COIN_LINE,
            coin_gap: phone::COIN_GAP,
            coin_align: Align::End,
            coin: TypeStyle {
                size: phone::COIN_LINE,
                ..COIN
            },
            title: TypeStyle {
                size: phone::TITLE,
                ..TITLE
            },
            title_gap: phone::TITLE_GAP,
            card,
            slot,
            card_gap: phone::GAP,
            slot_gap: phone::GAP,
            slots_gap: phone::SLOTS_GAP,
            hand_width: Some(per_row * card + (per_row - 1.0) * phone::GAP),
        }
    }
}
