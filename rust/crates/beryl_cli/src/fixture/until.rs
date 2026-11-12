//! Where a recording stops.

use beryl_session::protocol::Kind;
use beryl_session::{PlayerView, Seat, Status};
use serde_json::json;

/// Where a recording stops: the human's first decision of a kind, or first halt
/// (`DecisionView::halt`), or first view of the opponent handling cards while their decision
/// is open (`PlayerView::handling`: looking at their library), from a turn on, or the end of
/// the game.
#[derive(Debug, PartialEq)]
pub(super) enum Until {
    First { kind: Kind, seat: Seat, turn: u16 },
    Halt { seat: Seat, turn: u16 },
    Handling { seat: Seat, turn: u16 },
    GameOver,
}

impl Until {
    /// Parses "first <kind> decision for seat <n>", "first halt for seat <n>" or "first
    /// handling for seat <n>", optionally
    /// followed by "from turn <t>", with the kind as the protocol names it ("target",
    /// "chooseUpTo"), or "game over". A decision of a kind is the engine's, never a halt.
    pub(super) fn parse(text: &str) -> Result<Until, String> {
        let words: Vec<&str> = text.split_whitespace().collect();
        let (words, turn) = match words.as_slice() {
            [first @ .., "from", "turn", turn] => (
                first,
                turn.parse()
                    .map_err(|_| format!("{text:?}: no turn {turn:?}"))?,
            ),
            all => (all, 0),
        };
        let seat = |seat: &str| {
            seat.parse()
                .ok()
                .and_then(Seat::try_new)
                .ok_or_else(|| format!("no seat {seat:?}"))
        };
        match words {
            ["game", "over"] if turn == 0 => Ok(Until::GameOver),
            ["first", kind, "decision", "for", "seat", n] => Ok(Until::First {
                turn,
                kind: serde_json::from_value(json!(kind))
                    .map_err(|_| format!("no decision kind is named {kind:?}"))?,
                seat: seat(n)?,
            }),
            ["first", "halt", "for", "seat", n] => Ok(Until::Halt {
                turn,
                seat: seat(n)?,
            }),
            ["first", "handling", "for", "seat", n] => Ok(Until::Handling {
                turn,
                seat: seat(n)?,
            }),
            _ => Err(format!(
                "{text:?}: expected \"first <kind> decision for seat <n> [from turn <t>]\", \
                 \"first halt for seat <n> [from turn <t>]\", \"first handling for seat <n> \
                 [from turn <t>]\" or \"game over\""
            )),
        }
    }

    pub(super) fn seat(&self) -> Seat {
        match self {
            Until::First { seat, .. } | Until::Halt { seat, .. } | Until::Handling { seat, .. } => {
                *seat
            }
            Until::GameOver => Seat::P0,
        }
    }

    pub(super) fn reached(&self, view: &PlayerView) -> bool {
        let decision = match &view.status {
            Status::Deciding { decision } => Some(decision),
            _ => None,
        };
        match self {
            Until::First { kind, turn, .. } => {
                view.turn.number >= *turn && decision.is_some_and(|d| d.kind == *kind && !d.halt)
            }
            Until::Halt { turn, .. } => {
                view.turn.number >= *turn && decision.is_some_and(|d| d.halt)
            }
            Until::Handling { turn, .. } => view.turn.number >= *turn && view.handling.is_some(),
            Until::GameOver => matches!(view.status, Status::Over { .. }),
        }
    }
}
