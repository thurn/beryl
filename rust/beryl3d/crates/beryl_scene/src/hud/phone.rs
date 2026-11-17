//! The game screen's furniture on the phone layout (docs/ui-conventions.md, "The phone"), which
//! the web interface's `PhoneStageBoard` (`ui/src/stage/PhoneStageBoard.tsx`) and
//! `PhoneChrome.tsx` put in the table's bands (`table::phone_bands`):
//!
//! - The menu button at the right end of the opponent's strip, which opens the settings. The
//!   settings hold the log and the bug report on the phone, which has no room for the desktop's
//!   log column or its gear.
//! - The middle band: the phase icon on its line, at its left while the stack holds anything,
//!   and full control's mark at its right end, across from the phase.
//! - The log, as a sheet over the table while it is open.
//! - Where the pill with what a prompt asks stands (`asked_top`), which the prompt bar's phone
//!   composition reads (`prompts::prompt_bar`).

use beryl_protocol::protocol::{PlayerView, Side, Step};
use glam::Vec2;

use crate::data::space::hud::phone as data;
use crate::element::{Align, Anchor, Element, Justify, Press, Shown, column, row, text};
use crate::frame::Role;
use crate::input::Rect;
use crate::screens::{ScreenIn, Sheet};
use crate::table::PhoneBands;
use crate::tokens::{color, size, space, type_style};
use crate::widgets::{icon_button, icons};

use super::log;

/// Where the cards of the table rest once every script given so far has played, as the phone's
/// screen needs them: the boxes of the permanents and the stack's items on the window, and the
/// top of the viewer's hand, whose back row stands in their strip when the hand holds more cards
/// than one fan has room for. The client measures them as it builds the screen.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Occupied {
    /// The box on the window of each permanent and stack item, in logical points.
    pub cards: Vec<Rect>,
    /// The highest top edge of a card in the viewer's hand, in logical points from the
    /// window's top, if they hold any.
    pub hand_top: Option<f32>,
}

/// Nothing measured: the desktop layout, and screens built outside a client.
pub static NOTHING: Occupied = Occupied {
    cards: Vec::new(),
    hand_top: None,
};

/// Where the top of the pill with what the open prompt asks stands, `height` high, as the web's
/// `askedTop` (`ui/src/mobile/asked.ts`) puts it: centred on the middle band's line, as Arena
/// writes what a prompt asks across the middle of its table; or, where a card lies there (an
/// item on the stack), at the nearest height clear of every permanent and stack item, so that a
/// pick made on the battlefield shows every card it picks among. With no such height it stays
/// on the line. Heights are compared across the whole width, since the pill is centred and only
/// as wide as its words.
#[must_use]
pub fn asked_top(bands: &PhoneBands, occupied: &Occupied, height: f32) -> f32 {
    let line = bands.middle.top + (bands.middle.height - height) / 2.0;
    let gap = data::ASKED_GAP;
    let taken = || (occupied.cards.iter()).map(|card| (card.y, card.y + card.height));
    let clear = |top: f32| {
        top >= bands.opponent_back.top
            && top + height <= bands.your_strip.top
            && taken().all(|(card_top, card_bottom)| {
                top + height + gap <= card_top || top >= card_bottom + gap
            })
    };
    std::iter::once(line)
        .chain(taken().flat_map(|(top, bottom)| [bottom + gap, top - gap - height]))
        .filter(|&top| clear(top))
        .min_by(|a, b| (a - line).abs().total_cmp(&(b - line).abs()))
        .unwrap_or(line)
}

/// Whether a box `width` by `height` at the column's right end, its top at `top`, keeps clear
/// of every permanent and stack item, allowing `data::OVERLAP` points of overlap on each side,
/// so that a box that fills the middle band still fits between the creatures' rows, which a
/// token's pile reaches over by a few points.
#[must_use]
pub fn clear_at_right(
    bands: &PhoneBands,
    occupied: &Occupied,
    (width, height): (f32, f32),
    top: f32,
) -> bool {
    let spare = data::OVERLAP;
    let (left, bottom) = (bands.right - width + spare, top + height - spare);
    let (right, top) = (bands.right - spare, top + spare);
    occupied.cards.iter().all(|card| {
        card.x >= right
            || card.x + card.width <= left
            || card.y >= bottom
            || card.y + card.height <= top
    })
}

/// The menu's button, at the right end of the opponent's strip: it opens the settings, which
/// hold the log and the bug report (`MenuButton` in `ui/src/mobile/chrome.tsx`).
#[must_use]
pub fn menu(bands: &PhoneBands) -> Element {
    let mut button = icon_button(icons::MENU, "Menu").on_press(Press::Sheet(Some(Sheet::Settings)));
    button.anchor = Some(Anchor::Point(
        Vec2::new(bands.right, bands.opponent_strip.middle()),
        Vec2::new(1.0, 0.5),
    ));
    button
}

/// The middle band's furniture, as the web's `Middle` and `MiddleEnd` draw it: the phase icon on
/// the band's line, lit while it is the viewer's turn, centred, or at the band's left while the
/// stack holds anything; and full control's mark at the band's right end while it is on. Neither
/// takes a press: the phase is shown, and full control is set in the settings.
#[must_use]
pub fn middle(view: &PlayerView, full_control: bool, bands: &PhoneBands) -> Element {
    let yours = view.turn.active == Side::You;
    let side = f32::from(icons::PHASE_SIDE);
    let line = bands.middle.middle();
    let mut phase = Element::new(Role::Region)
        .name("phase")
        .width(side)
        .height(side)
        .radius(side / 2.0)
        .background(color::ICON_BUTTON)
        .color(if yours { color::GOLD } else { color::TEXT_DIM })
        .passes(true);
    phase.shows = Some(Shown::Icon(phase_icon(view.turn.step), icons::PHASE_SIDE));
    phase.anchor = Some(if view.stack.is_empty() {
        Anchor::Point(
            Vec2::new(f32::midpoint(bands.left, bands.right), line),
            Vec2::splat(0.5),
        )
    } else {
        Anchor::Point(Vec2::new(bands.left, line), Vec2::new(0.0, 0.5))
    });
    let mark = full_control.then(|| {
        let mut mark = Element::new(Role::Region)
            .name("full control")
            .width(side)
            .height(side)
            .color(color::GOLD)
            .passes(true);
        mark.shows = Some(Shown::Icon(icons::FULL_CONTROL, icon_side()));
        mark.anchor = Some(Anchor::Point(
            Vec2::new(bands.right, line),
            Vec2::new(1.0, 0.5),
        ));
        mark
    });
    crate::element::holder()
        .name("middle")
        .child(phase)
        .children(mark)
}

/// The icon of the phase `step` is in, of the web's five (`phaseOf` in `ui/src/mobile/layout.ts`):
/// beginning, first main, combat, second main and end.
fn phase_icon(step: Step) -> crate::widgets::IconId {
    let phase = match step {
        Step::Untap | Step::Upkeep | Step::Draw => 0,
        Step::PrecombatMain => 1,
        Step::BeginningOfCombat
        | Step::DeclareAttackers
        | Step::DeclareBlockers
        | Step::FirstStrikeDamage
        | Step::CombatDamage
        | Step::EndOfCombat => 2,
        Step::PostcombatMain => 3,
        Step::End | Step::Cleanup => 4,
    };
    icons::PHASES[phase]
}

/// The side of an icon button's glyph, as the atlas holds it.
fn icon_side() -> u16 {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a few points"
    )]
    let side = size::ICON as u16;
    side
}

/// The log as a sheet over the table while it is open (`LogSheet` in
/// `ui/src/stage/PhoneChrome.tsx`): the column's width, between the opponent's strip and the hand,
/// with its heading and a button that closes it over the list, which scrolls to its foot as the
/// desktop's does.
#[must_use]
pub fn log_sheet(input: &ScreenIn, bands: &PhoneBands) -> Option<Element> {
    input.view?;
    if !input.log_open {
        return None;
    }
    // Below the opponent's strip, so that the menu button stays clear of it.
    let margin = data::SHEET_MARGIN;
    let top = bands.opponent_strip.bottom() + margin;
    let height = (bands.hand.top - margin - top).max(0.0);
    let heading = row()
        .align_items(Align::Center)
        .justify_content(Justify::SpaceBetween)
        .child(text("Log").style(type_style::TITLE))
        .child(icon_button(icons::CLOSE, "Close log").on_press(Press::Log));
    let list = log::panel(input.log)
        .grow(1.0)
        .shrink(1.0)
        .min_height(size::ICON_BUTTON);
    let mut sheet = column()
        .name("log sheet")
        .width(bands.right - bands.left)
        .max_height(height)
        .padding(space::M)
        .gap(space::S)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(size::PANEL_RADIUS)
        .child(heading)
        .child(list);
    sheet.anchor = Some(Anchor::Point(Vec2::new(bands.left, top), Vec2::ZERO));
    Some(sheet)
}
