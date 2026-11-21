//! The opening hand and the mulligan, shown on the game screen before the first turn: the web
//! interface's `Opening` (`ui/src/screens/Opening.tsx`) with the rules of `.opening` in
//! `ui/src/screens/styles/opening.css`. A dimmed table, whether the viewer is on the play, a
//! title, the viewer's hand in a row of large cards and, while cards are being put on the
//! bottom of the library, a row of slots under it for them. The open prompt's buttons stand in
//! the prompt bar, which `screens::game` adds after this so that the backdrop is drawn under
//! them. The life orbs and the step stops stand under the backdrop, which dims them, as the
//! web's overlay is drawn over its `Hud`.
//!
//! The cards are the hand's scene objects (`Shown::Object`): the table moves each from the hand
//! into its box and back (`table::shown`), so no card is drawn at the hand while the opening is
//! shown. A put-on-bottom pick clicks the cards in the row (`Click::On`); the session marks each
//! one picked as selected, and the row shows it in a slot.
//!
//! The coin is the `announce` notice's float (`hud::notices`), which the client draws over
//! every screen.
//!
//! When the opponent won the flip, the opening first shows the flip for `FLIP_MS` however soon
//! they chose, as the web's coin spins (`HudIn::spinning`): the title says who won, the hand
//! is dealt out of sight and no button is offered. Until a button is offered, three gold dots
//! under the title say that the game is waiting, as the web's `.opening-waiting` does.
//!
//! Play or draw, asked before anyone has started, shows its buttons side by side under the title
//! (`choice`, the web's `.opening-choice`), and the game screen leaves the prompt bar out then
//! (`choosing_start`). The other opening buttons stand in the prompt bar.
//!
//! Not built yet: the web's face-down hand while the viewer has declared a mulligan.

use beryl_protocol::PlayerView;
use beryl_protocol::prompt::Click;
use beryl_protocol::prompt::{PromptButton, PromptView, Purpose};
use beryl_protocol::protocol::{CardHandling, Declaration, ObjectId, Side, Status, ZoneName};
use beryl_protocol::script::{Mark, Subject};
use glam::Vec2;

use crate::hud::HudIn;
use game_table::table::{CARD_HEIGHT, CARD_WIDTH, Window};
use kit_base::data::space::flat::opening as metrics;
use kit_base::data::space::flat::phone::opening as phone;
use kit_base::data::space::flat::phone::{ACTION_GAP, ACTION_HEIGHT, GUTTER};
use kit_base::tokens::{color, size, type_style};
use kit_frame::frame::Role;
use kit_text::text::TypeStyle;
use kit_ui::element::{
    Action, Align, Anchor, Element, Justify, Overflow, Shown, column, row, text,
};
use kit_ui::widgets::screen::backdrop;
use kit_ui::widgets::{button, primary};

/// The name of the element that holds the whole opening, its backdrop and its group, which
/// "every step shown" looks for (`agent::steps::sample`).
pub const SCREEN_NAME: &str = "opening screen";

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
pub(crate) fn in_opening(view: &PlayerView) -> bool {
    view.turn.number == 0
        && !matches!(
            view.status,
            Status::Free | Status::Over { .. } | Status::Stopped
        )
}

/// The view the opening shows: the latest whose script has played out, while it is in the
/// opening. The web's `Opening` is drawn from that view, so that its title and hand change only
/// as the table finishes playing the deal, a mulligan or the new hand, never ahead of it.
fn opening_view<'a>(input: &HudIn<'a>) -> Option<&'a PlayerView> {
    input.played.filter(|view| in_opening(view))
}

/// Whether the opening shows the open prompt's buttons itself, side by side under the title,
/// instead of the prompt bar: play or draw, asked before anyone has started, on the desktop
/// layout (`choosingStart` in `ui/src/screens/Opening.tsx`). While it does, the game screen
/// leaves the prompt out.
#[must_use]
pub(crate) fn choosing_start(input: &HudIn) -> bool {
    !input.window.phone()
        && !input.spinning
        && opening_view(input).is_some_and(|view| {
            view.turn.starting.is_none()
                && input.view.is_some_and(|latest| std::ptr::eq(latest, view))
        })
        && input
            .prompt
            .is_some_and(|prompt| !prompt.buttons.is_empty())
}

/// Whether the game screen shows the opening.
#[must_use]
pub fn shown(input: &HudIn) -> bool {
    opening_view(input).is_some()
}

/// What the game screen shows in the opening, or none once it is over.
#[must_use]
pub fn opening(input: &HudIn) -> Option<Element> {
    let view = opening_view(input)?;
    let you = &view.players.you;
    // While the coin spins, the opening does not show who starts, as the web's `Opening` holds
    // it back; the client offers no decision meanwhile.
    let starting = view.turn.starting.filter(|_| !input.spinning);
    // The open prompt belongs to the latest view, so while scripts before it still play the
    // opening offers no decision, as the web's offers none until its view has played out.
    let caught_up = input.view.is_some_and(|latest| std::ptr::eq(latest, view));
    let prompt = input.prompt.filter(|_| caught_up);
    let picking = prompt.filter(|prompt| prompt.is(Purpose::PutOnBottom));
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
    let title = title(view, starting, picking.is_some());
    let actions = if picking.is_some() {
        1
    } else {
        prompt.map_or(0, |prompt| prompt.buttons.len())
    };
    let shown: Vec<ObjectId> = (hand.iter().copied())
        .filter(|&slot| !picked(slot))
        .collect();
    // The opponent choosing who starts shows on the coin, and the opponent considering their
    // hand on their side of the table, so neither shows the dots (`Opening`'s
    // `choosingFirst` and `considering`).
    let choosing_first =
        starting.is_none() && view.turn.flip_winner == Some(Side::Opponent) && !input.spinning;
    let considering = matches!(view.handling, Some(CardHandling::Hand { .. }));
    Some(present(
        &Presentation {
            view,
            starting,
            title: &title,
            hand: &shown,
            hand_hidden: starting.is_none(),
            rows: hand.len(),
            bottom: &bottom,
            slots: to_bottom.max(bottom.len()),
            actions,
            waiting: actions == 0 && !choosing_first && !considering,
            press: None,
            choice: if choosing_start(input) {
                prompt.map_or(&[], |prompt| &prompt.buttons)
            } else {
                &[]
            },
        },
        input.window,
    ))
}

/// What an opening shows: whether the viewer is on the play (none while that is still being
/// chosen), the title, the cards still in the hand row and those on their way to the bottom,
/// how many cards the hand holds in all (which sizes the phone's rows), how many slots the
/// bottom row has, how many buttons the dock holds, and what a press on a card does, if
/// anything. The engine's game and the manual table each fill it from their own prompt.
///
/// `hand_hidden` lays the hand row out but draws none of its cards, as the web keeps the hand
/// under the table until it knows who starts, so that the cards are dealt out of sight and then
/// stand in their places at once. `waiting` shows the dots under the title.
pub(crate) struct Presentation<'a> {
    pub view: &'a PlayerView,
    pub starting: Option<Side>,
    pub title: &'a str,
    pub hand: &'a [ObjectId],
    pub hand_hidden: bool,
    pub rows: usize,
    pub bottom: &'a [ObjectId],
    pub slots: usize,
    pub actions: usize,
    pub waiting: bool,
    pub press: Option<fn(ObjectId) -> Action>,
    /// The buttons shown side by side under the title (`.opening-choice`): the first is
    /// primary and stands rightmost, as in Arena.
    pub choice: &'a [PromptButton],
}

/// The opening `shown` describes, over the dimmed table: the coin, the title, the hand and the
/// bottom slots, as the web's `OpeningPresentation` lays them out.
pub(crate) fn present(shown: &Presentation, window: Window) -> Element {
    let Presentation {
        view,
        starting,
        slots,
        ..
    } = *shown;
    let sizes = if window.phone() {
        // Room for both rows while cards move between the hand and the slots, as the web
        // keeps it, and for the prompt's buttons in the dock under them.
        let rows = shown.rows.div_ceil(phone::PER_ROW);
        Sizes::phone(window, rows, slots, shown.actions)
    } else {
        Sizes::DESKTOP
    };
    let card = |slot: ObjectId, width: f32| {
        let mut card = card_box(view, slot, width);
        card.press = shown.press.map(|press| press(slot));
        card
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
    if shown.hand_hidden {
        // The cards' boxes lie outside this empty strip's clip, so the table draws none of
        // them (`Layer::shown_objects`), and the strip takes no room, as the web's empty
        // `.opening-hand` takes none.
        hand_row = row()
            .name("hand held")
            .height(0.0)
            .overflow(Overflow::Clip)
            .passes(true)
            .child(hand_row);
    }
    let dots = (shown.waiting && !window.phone()).then(waiting);
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
    let passes = shown.press.is_none() && shown.choice.is_empty();
    let mut screen = column()
        .name("opening")
        .passes(passes)
        .align_items(Align::Center)
        .child(coin)
        .child(heading)
        .child(hand_row)
        .children(slots_row)
        .children(dots)
        .children(choice(shown.choice));
    // The flat layer's root puts a screen in its bottom right corner, so the group is anchored
    // below the window's top middle instead, as the web pads `.opening` down from the top.
    let top = Vec2::new(window.size.width / 2.0, sizes.top);
    screen.anchor = Some(Anchor::Point(top, Vec2::new(0.5, 0.0)));
    row()
        .name(SCREEN_NAME)
        .passes(passes)
        // The opening's backdrop is a gradient that leaves the opponent's side of the table
        // clear, in place of the backdrop's flat color.
        .child(
            backdrop(window.size)
                .background([0; 4])
                .background_image(vec![kit_base::tokens::gradient::OPENING_BACKDROP]),
        )
        .child(screen)
}

/// The play or draw buttons, `.opening-choice`: a row reversed so that the first button stands
/// on the right, 40 points apart (so the children are laid out last to first), each at least
/// `--button-width` wide, 40 points under what precedes them.
fn choice(buttons: &[PromptButton]) -> Option<Element> {
    if buttons.is_empty() {
        return None;
    }
    let mut choice = row()
        .name("choice")
        .gap(metrics::CHOICE_GAP)
        .justify_content(Justify::Center);
    choice.style.margin = [metrics::CHOICE_TOP, 0.0, 0.0, 0.0];
    Some(
        choice.children(buttons.iter().enumerate().rev().map(|(at, view)| {
            let widget = if at == 0 { primary } else { button };
            widget(&view.label)
                .min_width(size::BUTTON_WIDTH)
                .enabled(view.enabled)
                .on_press(Click::Button { button: view.id })
        })),
    )
}

/// The three gold dots under the title while the game waits for the session or the opponent,
/// `.opening-buttons` holding `.working-dots.opening-waiting`: 34-point type whose dots are
/// 0.26em wide, 0.22em apart, 0.3em in from the row's start. The web pulses them; these stand
/// still.
fn waiting() -> Element {
    let dot = |at: usize| {
        row()
            .name(&format!("dot {}", at + 1))
            .width(metrics::DOT)
            .height(metrics::DOT)
            .radius(metrics::DOT / 2.0)
            .background(color::GOLD)
            .passes(true)
    };
    let mut dots = row()
        .name("working")
        .gap(metrics::DOT_GAP)
        .passes(true)
        .children((0..3).map(dot));
    dots.style.margin = [0.0, 0.0, 0.0, metrics::DOT_INSET];
    let mut buttons = row()
        .name("waiting")
        .min_height(metrics::WAITING_HEIGHT)
        .align_items(Align::Center)
        .passes(true)
        .child(dots);
    buttons.style.margin = [metrics::WAITING_GAP, 0.0, 0.0, 0.0];
    buttons
}

/// The title, as the web's `Opening` words it, given who starts as the opening shows it.
fn title(view: &PlayerView, starting: Option<Side>, picking: bool) -> String {
    let you = &view.players.you;
    match starting {
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
