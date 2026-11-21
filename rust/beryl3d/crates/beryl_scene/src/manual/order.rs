//! The manual table's library order (docs/manual-multiplayer.md, "Drag"): when several cards
//! are put on a library at once, a tray holds them up in a row, the top card first, so that the
//! player chooses their order before they go. A press on a card moves it to the top, the left
//! and right arrow keys move the highlighted card along the row, Done puts the cards on the
//! library in that order and Cancel leaves them where they were. This is the web's
//! `LibraryOrder` (`ui/src/table/LibraryOrder.tsx`). Each card is its scene object shown in a
//! box of the flat layer (`Shown::Object`), so the table lifts it into the tray and lets it go
//! with the tray's own timing, as the pile viewer's cards move.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{ObjectId, Side, TableAction};
use beryl_protocol::script::Subject;

use super::TablePress;
use super::commands::seat_of;
use crate::element::{Element, Press, row};
use crate::input::Size;
use crate::inspect::viewer::object_box;
use crate::tokens::size;
use crate::widgets::screen::overlay;
use crate::widgets::{button, primary};

/// The tray's title.
pub const TITLE: &str = "Order Cards";

/// The cards being ordered onto `side`'s library, the top card first, and the one the arrow
/// keys move.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LibraryOrder {
    pub side: Side,
    pub cards: Vec<ObjectId>,
    pub highlight: usize,
}

impl LibraryOrder {
    /// The tray for `cards`, in the order they were put down, going onto `side`'s library.
    #[must_use]
    pub fn new(side: Side, cards: Vec<ObjectId>) -> Self {
        Self {
            side,
            cards,
            highlight: 0,
        }
    }

    /// Moves the card at `from` to `to`, the cards between closing up, and highlights it.
    pub fn shift(&mut self, from: usize, to: usize) {
        if from >= self.cards.len() || to >= self.cards.len() {
            return;
        }
        let card = self.cards.remove(from);
        self.cards.insert(to, card);
        self.highlight = to;
    }

    /// Moves the highlighted card one place toward the bottom, or toward the top if not
    /// `later`.
    pub fn nudge(&mut self, later: bool) {
        let at = self.highlight;
        let to = if later { at + 1 } else { at.wrapping_sub(1) };
        self.shift(at, to);
    }

    /// The actions that put the cards on the library in the tray's order: one move to the top
    /// for each card, the bottom card first, so that the first card ends on top. Empty when a
    /// card has no reference in `view`.
    #[must_use]
    pub fn actions(&self, view: &PlayerView) -> Vec<TableAction> {
        let Some(table) = &view.table else {
            return Vec::new();
        };
        let seat = seat_of(view, self.side);
        let moves: Option<Vec<TableAction>> = (self.cards.iter().rev())
            .map(|&slot| {
                let action = (table.object_actions.iter())
                    .find(|action| action.subject == Subject::Object { slot })?;
                let json = serde_json::json!({ "move": {
                    "objects": [action.reference],
                    "in": action.zone,
                    "to": { "library": seat },
                }});
                serde_json::from_value(json).ok()
            })
            .collect();
        moves.unwrap_or_default()
    }
}

/// The tray over a window of `window`'s size: its title, the cards in a row with the
/// highlighted one marked, Done and Cancel.
#[must_use]
pub fn tray(order: &LibraryOrder, view: &PlayerView, window: Size) -> Element {
    let cards =
        row()
            .name("cards")
            .gap(size::VIEWER_GAP)
            .children((order.cards.iter().enumerate()).map(|(at, &slot)| {
                let at_u16 = u16::try_from(at).unwrap_or(u16::MAX);
                let mut card = object_box(view, slot)
                    .selected(at == order.highlight)
                    .on_press(Press::Table(TablePress::OrderCard(at_u16)));
                card.focus = true;
                card
            }));
    let done = primary("Done")
        .name("done")
        .on_press(Press::Table(TablePress::OrderDone));
    let cancel = button("Cancel")
        .name("close")
        .on_press(Press::Table(TablePress::Close));
    overlay(window, (TITLE, None), None, cards, vec![done, cancel])
}
