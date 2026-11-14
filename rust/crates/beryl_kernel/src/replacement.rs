//! Replacement effects of static abilities: which of them the engine applies.
//!
//! A replacement says what it replaces ([`Replaced`]) and what happens instead
//! ([`Rewrite`]). [`check`] accepts the pairs the rules have code for and refuses the rest, and
//! a game's card table is built only from cards it accepts
//! ([`crate::table::TableError::Replacement`]). The rules then read what was accepted, with no
//! case for what was refused.
//!
//! A replacement of a draw says what happens instead as an [`Effect`], which says more than
//! the engine does in place of a draw: [`instead_of_drawing`] gives what the rules do or
//! refuses the ability.

use beryl_card::{
    Ability, Behavior, CardDef, Effect, Players, Replaced, Replacement, Rewrite, Zone,
};

use crate::event::{ReplacementKind, Replaces};
use crate::filter::reads;

/// Why the engine cannot apply a replacement effect: the part of it no code reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unreplaced {
    /// The rules have no code for this rewrite of this event.
    Rewrite,
    /// A draw is replaced by something other than a player winning or losing the game.
    Instead,
    /// Players are read for other than "you", "each opponent" and "each player": a static
    /// ability has no targets, and nothing before it to name one.
    Players,
}

/// What a replacement of a draw has happen instead, as the engine does it.
#[derive(Clone, Copy, Debug)]
pub enum Instead {
    /// These players win the game.
    Win(Players),
    /// These players lose the game.
    Lose(Players),
}

/// What `ability`, a replacement of draws ([`Replaced::Draw`]), has happen instead of a draw,
/// or why the engine cannot do it. The players are read for the permanent's controller
/// ([`crate::filter::is_for`]).
pub fn instead_of_drawing(ability: &Ability) -> Result<Instead, Unreplaced> {
    let (instead, players) = match ability.body() {
        Behavior::Data(&Effect::WinGame(players)) => (Instead::Win(players), players),
        Behavior::Data(&Effect::LoseGame(players)) => (Instead::Lose(players), players),
        _ => return Err(Unreplaced::Instead),
    };
    match players {
        Players::You | Players::EachOpponent | Players::EachPlayer => Ok(instead),
        _ => Err(Unreplaced::Players),
    }
}

/// Whether `ability` replaces draws.
#[inline]
pub fn replaces_draws(ability: &Ability) -> bool {
    matches!(
        ability.replacement(),
        Some(Replacement {
            replaces: Replaced::Draw(_),
            ..
        })
    )
}

/// The kinds of event the static abilities of a card's faces replace ([`Replaces`]), or the
/// first replacement of its faces the engine cannot apply. A replacement of how its own
/// permanent enters is found from the entering object's face, so it adds no kind.
pub(crate) fn check(def: &CardDef) -> Result<Replaces, Unreplaced> {
    let mut mask = Replaces::NONE;
    for ability in def.faces.iter().flat_map(|face| face.abilities) {
        let Some(&Replacement { replaces, rewrite }) = ability.replacement() else {
            continue;
        };
        match (replaces, rewrite) {
            (
                Replaced::ThisEnters,
                Rewrite::EntersTapped
                | Rewrite::EntersTappedUnlessFewLands(_)
                | Rewrite::EntersTappedUnlessPayLife(_),
            ) => {}
            (Replaced::Draw(players), Rewrite::Instead) => {
                if !reads(&players) {
                    return Err(Unreplaced::Players);
                }
                instead_of_drawing(ability)?;
                mask = mask | ReplacementKind::Draw;
            }
            (Replaced::PutInto(_, zone), Rewrite::ToZone(instead)) if zone != instead => {
                mask = mask | ReplacementKind::Move;
            }
            // Entering is a move to the battlefield, replaced as the move's destination is.
            (Replaced::Enters { .. }, Rewrite::ToZone(instead)) if instead != Zone::Battlefield => {
                mask = mask | ReplacementKind::Move;
            }
            _ => return Err(Unreplaced::Rewrite),
        }
    }
    Ok(mask)
}
