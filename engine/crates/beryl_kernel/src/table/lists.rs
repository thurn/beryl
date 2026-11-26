//! Card indices from the game’s original lists.
//!
//! Implements: none. Indexes main decks, sideboards and the cards they can create.

use beryl_card::definition::Shape;
use beryl_card::{CardDef, FACE_DOWN_COPY};

use crate::format::{DeckRole, Format};
use crate::ids::Seat;
use crate::layout::PerSeat;

use super::{CardTable, Decklist, TableError};

/// One list of game indices for each player.
type Indices = PerSeat<Vec<usize>>;

/// The game indices of each player's deck and sideboard, in the roles `format` gives its lists
/// ([`Format::decks`]). In the shared-deck format the first deck is the shared library, and a
/// second deck or a sideboard is refused.
pub(super) fn gather_lists(
    format: Format,
    cards: &'static CardTable,
    decks: PerSeat<&Decklist>,
    sideboards: PerSeat<&Decklist>,
    defs: &mut Vec<&'static CardDef>,
) -> Result<(Indices, Indices), TableError> {
    let count = |list: &Decklist| list.iter().map(|&(n, _)| usize::from(n)).sum::<usize>();
    let sideboard_fits = || {
        for (player, list) in Seat::BOTH.into_iter().zip(sideboards) {
            let count = count(list);
            if count > format.maximum_sideboard() {
                return Err(TableError::SideboardTooLarge { player, count });
            }
        }
        Ok(())
    };
    let mut indices = Default::default();
    let mut outside = Default::default();
    for &role in format.decks() {
        match role {
            DeckRole::Library => indices = gather_role(cards, decks, defs)?,
            DeckRole::Sideboard => {
                sideboard_fits()?;
                outside = gather_role(cards, sideboards, defs)?;
            }
            DeckRole::SharedLibrary => {
                let second = count(decks[Seat::P1.index()]);
                if second > 0 {
                    return Err(TableError::SecondDeck { count: second });
                }
                sideboard_fits()?;
                indices = gather_role(cards, decks, defs)?;
            }
        }
    }
    Ok((indices, outside))
}

/// The game indices of the two lists of a role, counted in decklist order.
fn gather_role(
    cards: &'static CardTable,
    lists: PerSeat<&Decklist>,
    defs: &mut Vec<&'static CardDef>,
) -> Result<Indices, TableError> {
    let mut indices: Indices = Default::default();
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

/// Index the tokens and emblems the cards can create and the combined back faces their meld
/// cards meld into, then the extra definitions. A meld card whose result `cards` does not
/// have can't meld in this game (see CR 701.42c), so the game is played without it.
pub(super) fn gather_extra(
    defs: &mut Vec<&'static CardDef>,
    cards: &'static CardTable,
    extra: &[&'static CardDef],
) {
    let mut i = 0;
    while i < defs.len() {
        let melded = match defs[i].shape {
            Shape::Meld { into, .. } => cards.get(into),
            Shape::Single
            | Shape::Transforming
            | Shape::Modal
            | Shape::Split { .. }
            | Shape::Adventure
            | Shape::Prepared
            | Shape::Flip
            | Shape::Melded => None,
        };
        for &made in defs[i].tokens.iter().chain(melded.as_ref()) {
            if !defs.iter().any(|d| std::ptr::eq(*d, made)) {
                defs.push(made);
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
