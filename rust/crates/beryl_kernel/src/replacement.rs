//! Replacement effects of static abilities that replace an event other than their own
//! permanent's entering: which of them the engine applies.
//!
//! A replacement of a draw ([`Replacement::WouldDraw`]) says what happens instead as an
//! [`Effect`], which says more than the engine does in place of a draw. What it does is
//! said here, in one place: [`instead_of_drawing`] gives what the rules do or refuses the
//! ability, and a game's card table is built only from abilities it gives
//! ([`crate::table::TableError::Replacement`]). The rules then read what was given, with no
//! case for what was refused.

use beryl_card::{Ability, AbilityKind, Behavior, CardDef, Effect, Players, Replacement};

use crate::event::kind;

/// Why the engine cannot apply a replacement effect: the part of it no code reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unreplaced {
    /// A draw is replaced by something other than a player winning or losing the game.
    Instead,
    /// What happens instead is read for players other than "you", "each opponent" and
    /// "each player": a static ability has no targets, and nothing before it to name one.
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

/// What `ability`, a replacement of its controller's draws ([`Replacement::WouldDraw`]), has
/// happen instead of a draw, or why the engine cannot do it. The players are read for the
/// permanent's controller ([`crate::listener::is_for`]).
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

/// Whether `ability` replaces its controller's draws.
#[inline]
pub fn replaces_draws(ability: &Ability) -> bool {
    ability.kind() == AbilityKind::Replacement && ability.replacement() == Replacement::WouldDraw
}

/// The bit of a card's listener mask that says a static ability of it replaces its
/// controller's draws ([`kind::DRAW_REPLACEMENT`]), if one does, or the first replacement
/// of its faces the engine cannot apply.
pub(crate) fn check(def: &CardDef) -> Result<u16, Unreplaced> {
    let mut mask = 0;
    for ability in def.faces.iter().flat_map(|face| face.abilities) {
        if replaces_draws(ability) {
            instead_of_drawing(ability)?;
            mask |= kind::DRAW_REPLACEMENT;
        }
    }
    Ok(mask)
}
