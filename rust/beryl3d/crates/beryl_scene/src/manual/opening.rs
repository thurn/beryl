//! The guided start of a manual table (docs/manual-multiplayer.md): the seat that won the roll
//! for first, was named, or lost the previous game chooses play or draw, then both seats see
//! their opening hands at once and keep or mulligan, putting cards on the bottom after a
//! mulligan. The table holds each seat's prompt (`TablePrompt::Opening`, or
//! `TablePrompt::Choose` while `TableView::choosing_first`), and the player answers it with
//! `Answer`s, as the web's `TableOpening` (`ui/src/table/TableOpening.tsx`) sends them. What it
//! shows is `screens::table_opening`.

use beryl_protocol::PlayerView;
use beryl_protocol::protocol::{Answer, ObjectId, TablePrompt};

/// A press on the guided start's screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpeningPress {
    /// The `n`th option of the play or draw choice.
    Pick(u16),
    /// Skip the choice, which leaves who starts to the table.
    Skip,
    Keep,
    Mulligan,
    /// Put this card on the bottom, or take it back into the hand if it is there already.
    Bottom(ObjectId),
    /// Confirm the cards put on the bottom.
    Done,
}

impl OpeningPress {
    /// The answers the press sends, in order: picking an option also confirms it, as the web
    /// sends `pick` then `done`.
    #[must_use]
    pub fn answers(self) -> Vec<Answer> {
        match self {
            Self::Pick(option) => vec![
                Answer::Pick {
                    option: usize::from(option),
                },
                Answer::Done,
            ],
            Self::Skip => vec![Answer::Cancel],
            Self::Keep => vec![Answer::Keep],
            Self::Mulligan => vec![Answer::Mulligan],
            Self::Bottom(card) => vec![Answer::Bottom { card }],
            Self::Done => vec![Answer::Done],
        }
    }
}

/// The guided start's prompt the viewer has open in `view`, if any: the play or draw choice
/// while the table waits on it, or the viewer's opening hand.
#[must_use]
pub fn prompt(view: &PlayerView) -> Option<&TablePrompt> {
    let table = view.table.as_ref()?;
    match table.prompt.as_ref()? {
        prompt @ TablePrompt::Opening { .. } => Some(prompt),
        prompt @ TablePrompt::Choose { .. } if table.choosing_first => Some(prompt),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picking_play_or_draw_confirms_the_pick() {
        assert_eq!(
            OpeningPress::Pick(1).answers(),
            vec![Answer::Pick { option: 1 }, Answer::Done]
        );
    }
}
