//! Card indices from the game’s original lists.
//!
//! Implements: none. Indexes main decks, sideboards and the cards they can create.

use beryl_card::{CardDef, FACE_DOWN_COPY};

use crate::ids::Seat;
use crate::layout::PerSeat;

use super::{CardTable, Decklist, TableError};

/// The game indices of the two lists of a role, counted in decklist order.
pub(super) fn gather_role(
    cards: &'static CardTable,
    lists: PerSeat<&Decklist>,
    defs: &mut Vec<&'static CardDef>,
) -> Result<PerSeat<Vec<usize>>, TableError> {
    let mut indices: PerSeat<Vec<usize>> = Default::default();
    for (player, list) in Seat::BOTH.into_iter().zip(lists) {
        for &(count, name) in list {
            let def = cards.get(name).ok_or(TableError::UnknownCard(name))?;
            let index = if let Some(i) = defs.iter().position(|d| std::ptr::eq(*d, def)) {
                i
            } else {
                defs.push(def);
                defs.len() - 1
            };
            indices[player.index()].extend(std::iter::repeat_n(index, usize::from(count)));
        }
    }
    Ok(indices)
}

/// Index the tokens and emblems the cards can create, then the extra definitions.
pub(super) fn gather_extra(defs: &mut Vec<&'static CardDef>, extra: &[&'static CardDef]) {
    let mut i = 0;
    while i < defs.len() {
        for &token in defs[i].tokens {
            if !defs.iter().any(|d| std::ptr::eq(*d, token)) {
                defs.push(token);
            }
        }
        i += 1;
    }
    for &def in extra {
        if !defs.iter().any(|d| std::ptr::eq(*d, def)) {
            defs.push(def);
        }
    }
    defs.push(&FACE_DOWN_COPY);
}
