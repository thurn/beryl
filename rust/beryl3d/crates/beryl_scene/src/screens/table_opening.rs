//! What the manual table shows during its guided start (`manual::opening`), as the web's
//! `TableOpening` (`ui/src/table/TableOpening.tsx`) composes the opening it shares with the
//! game (`super::opening::present`):
//!
//! - While the viewer chooses play or draw, the prompt's title and its options with Skip after
//!   them, side by side in the middle of the window on the desktop, as Arena asks, and in the
//!   dock on the phone. No hand is shown yet.
//! - Then the opening hand, whether the viewer is on the play, and Keep and Mulligan in the
//!   dock; after a mulligan, the slots for the cards to put on the bottom, which a press on a
//!   card fills or empties, and Done, which can be pressed with none chosen or all of them.
//!
//! The roll for first is the step banner's (`hud::notices`), which names each seat's die.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ObjectId, TablePrompt};
use glam::Vec2;

use super::opening::{Presentation, present};
use crate::manual::TablePress;
use crate::manual::opening::{OpeningPress, prompt};
use crate::table::Window;
use kit_base::tokens::{color, size, space};
use kit_ui::element::{Action, Align, Anchor, Element, column, row};
use kit_ui::widgets::screen::phone_action;
use kit_ui::widgets::{button, primary};

/// The press on a card of the opening hand: it goes on the bottom, or comes back.
fn bottom(slot: ObjectId) -> Action {
    Action::new(TablePress::Opening(OpeningPress::Bottom(slot)))
}

/// A button that sends `press`, the primary button if `first`.
fn answer(label: &str, press: OpeningPress, first: bool) -> Element {
    let widget = if first { primary } else { button };
    widget(label).on_press(TablePress::Opening(press))
}

/// The guided start's screen and its buttons, which stand beside it rather than in it so that
/// they never inherit its passing the pointer through; empty while the viewer has no prompt of
/// it open.
#[must_use]
pub(super) fn table_opening(view: Option<&PlayerView>, window: Window) -> Vec<Element> {
    view.and_then(|view| pieces(view, window))
        .map_or_else(Vec::new, |(shown, actions)| vec![shown, actions])
}

/// The guided start's screen and its buttons, as `table_opening` returns them.
fn pieces(view: &PlayerView, window: Window) -> Option<(Element, Element)> {
    let phone = window.phone();
    let (shown, buttons, middle) = match prompt(view)? {
        TablePrompt::Choose { title, options, .. } => {
            let mut buttons: Vec<Element> = (options.iter().enumerate())
                .map(|(at, label)| {
                    let at = u16::try_from(at).unwrap_or(u16::MAX);
                    answer(label, OpeningPress::Pick(at), at == 0)
                })
                .collect();
            buttons.push(answer("Skip", OpeningPress::Skip, false));
            let shown = Presentation {
                view,
                starting: None,
                title,
                hand: &[],
                hand_hidden: false,
                rows: 0,
                bottom: &[],
                slots: 0,
                actions: buttons.len(),
                waiting: false,
                press: None,
                choice: &[],
            };
            (present(&shown, window), buttons, !phone)
        }
        TablePrompt::Opening {
            hand,
            mulligans,
            to_bottom,
            bottom: chosen,
        } => {
            let picking = to_bottom.is_some();
            let title = if picking {
                "Put on the bottom".to_owned()
            } else if *mulligans == 0 {
                "Opening hand".to_owned()
            } else {
                format!(
                    "Mulligan to {}",
                    hand.len().saturating_sub(usize::from(*mulligans))
                )
            };
            let buttons = if picking {
                let wanted = usize::from(to_bottom.unwrap_or(0));
                let done = chosen.is_empty() || chosen.len() == wanted;
                vec![answer("Done", OpeningPress::Done, true).enabled(done)]
            } else {
                vec![
                    answer("Keep", OpeningPress::Keep, true),
                    answer("Mulligan", OpeningPress::Mulligan, false),
                ]
            };
            let in_hand: Vec<ObjectId> = (hand.iter().copied())
                .filter(|slot| !chosen.contains(slot))
                .collect();
            let shown = Presentation {
                view,
                starting: Some(view.turn.active),
                title: &title,
                hand: &in_hand,
                hand_hidden: false,
                rows: hand.len(),
                bottom: chosen,
                slots: usize::from(to_bottom.unwrap_or(0)).max(chosen.len()),
                actions: buttons.len(),
                waiting: false,
                press: picking.then_some(bottom as fn(ObjectId) -> Action),
                choice: &[],
            };
            (present(&shown, window), buttons, false)
        }
        TablePrompt::Sort { .. } => return None,
    };
    let actions = if middle {
        middle_row(buttons, window)
    } else if phone {
        phone_dock(buttons, window)
    } else {
        dock(buttons, window)
    };
    Some((shown, actions))
}

/// The play or draw buttons side by side in the middle of the window, `.opening-choice`.
fn middle_row(buttons: Vec<Element>, window: Window) -> Element {
    let mut choice = row().name("choice").gap(space::M).children(buttons);
    let middle = Vec2::new(window.size.width, window.size.height) / 2.0;
    choice.anchor = Some(Anchor::Point(middle, Vec2::splat(0.5)));
    choice
}

/// The buttons in the dock at the window's lower right, as the table's ask prompt stands.
fn dock(buttons: Vec<Element>, window: Window) -> Element {
    let window = window.size;
    let mut dock = row()
        .name("opening actions")
        .gap(space::M)
        .padding(space::L)
        .align_items(Align::Center)
        .background(color::PANEL)
        .border((color::METAL, size::BORDER))
        .radius(size::PANEL_RADIUS)
        .children(buttons);
    let inset = kit_base::data::space::flat::table::EDGE;
    let corner = Vec2::new(window.width - inset, window.height - inset);
    dock.anchor = Some(Anchor::Point(corner, Vec2::ONE));
    dock
}

/// The buttons stacked across the foot of the phone's safe area, as the web's phone dock
/// stands them.
fn phone_dock(buttons: Vec<Element>, window: Window) -> Element {
    use kit_base::data::space::flat::phone::{ACTION_GAP, GUTTER};
    let safe = window.safe_area;
    let width = window.size.width - safe.left - safe.right - 2.0 * GUTTER;
    let mut dock = column()
        .name("opening actions")
        .gap(ACTION_GAP)
        .width(width)
        .children(buttons.into_iter().map(phone_action));
    let foot = Vec2::new(
        safe.left + GUTTER + width / 2.0,
        window.size.height - safe.bottom - GUTTER,
    );
    dock.anchor = Some(Anchor::Point(foot, Vec2::new(0.5, 1.0)));
    dock
}
