//! Replacement effects of static abilities: which of them the engine applies.
//!
//! A replacement says what it replaces ([`Replaced`]) and what happens instead
//! ([`Rewrite`]). [`check`] accepts the pairs the rules have code for and refuses the rest, and
//! a game's card table is built only from cards it accepts
//! ([`crate::table::TableError::Replacement`]). The rules then read what was accepted, with no
//! case for what was refused.
//!
//! A replacement of a draw says what happens instead as an [`Effect`]: a player winning or
//! losing the game, which the rules do directly, or instructions the rules carry out as a
//! resolution would ([`instead_of_drawing`]).
//!
//! Implements: none. Checks whether the engine supports a replacement effect definition.

use beryl_card::{
    Ability, Behavior, CardDef, Draws, Effect, Players, Replaced, Replacement, Rewrite, Zone,
};

use crate::event::{ReplacementKind, Replaces};
use crate::filter::reads;

/// Why the engine cannot apply a replacement effect: the part of it no code reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unreplaced {
    /// The rules have no code for this rewrite of this event.
    Rewrite,
    /// A draw is replaced by a Rust body, which the rules cannot run in place of a draw.
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
    /// The ability's body is carried out, as a resolution carries out instructions, with
    /// "that player" the player who would have drawn: "that player discards a card instead".
    Instructions,
}

/// What `ability`, a replacement of draws ([`Replaced::Draw`]), has happen instead of a draw,
/// or why the engine cannot do it. The players are read for the permanent's controller
/// ([`crate::filter::is_for`]).
pub fn instead_of_drawing(ability: &Ability) -> Result<Instead, Unreplaced> {
    let (instead, players) = match ability.body() {
        Behavior::Data(&Effect::WinGame(players)) => (Instead::Win(players), players),
        Behavior::Data(&Effect::LoseGame(players)) => (Instead::Lose(players), players),
        Behavior::Data(_) => return Ok(Instead::Instructions),
        Behavior::Custom(_) => return Err(Unreplaced::Instead),
    };
    match players {
        Players::You | Players::EachOpponent | Players::EachPlayer => Ok(instead),
        Players::Target(_)
        | Players::ControllerOf(_)
        | Players::OwnerOf(_)
        | Players::Enchanted
        | Players::ThatPlayer
        | Players::SubgameNonwinners
        | Players::EachOther(_) => Err(Unreplaced::Players),
    }
}

/// Whether `ability` replaces draws.
#[inline]
pub fn replaces_draws(ability: &Ability) -> bool {
    ability.replaces_draws()
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
                | Rewrite::EntersPrepared
                | Rewrite::EntersTappedUnlessFewLands(_)
                | Rewrite::EntersTappedUnlessYouControl(..)
                | Rewrite::EntersTappedUnlessPayLife(_)
                | Rewrite::EntersWithCounters(..)
                | Rewrite::Choose(_)
                | Rewrite::EnterAsCopy { .. },
            ) => {}
            (Replaced::Draw(players, _), Rewrite::Instead) => {
                if !reads(&players) {
                    return Err(Unreplaced::Players);
                }
                mask = mask | ReplacementKind::Draw;
                if let Instead::Instructions = instead_of_drawing(ability)? {
                    mask = mask | ReplacementKind::DrawInstructions;
                }
            }
            (Replaced::PutInto(_, zone), Rewrite::ToZone(instead)) if zone != instead => {
                mask = mask | ReplacementKind::Move;
            }
            // Entering is a move to the battlefield, replaced as the move's destination is.
            (Replaced::Enters { .. }, Rewrite::ToZone(instead)) if instead != Zone::Battlefield => {
                mask = mask | ReplacementKind::Move;
            }
            // Dredge works from a graveyard, not the battlefield, so it adds no kind to the
            // mask the layers fold over permanents: the game's table says whether any card has
            // it ([`crate::table::Capability::Dredge`]).
            (Replaced::Draw(Players::You, Draws::Each), Rewrite::Dredge(n)) if n > 0 => {}
            (Replaced::BeginTurn, Rewrite::MaySkipTurnToUntap) => {
                mask = mask | ReplacementKind::BeginTurn;
            }
            (Replaced::Damage, Rewrite::Multiply(n)) if n > 1 => {
                mask = mask | ReplacementKind::Damage;
            }
            (Replaced::Tokens { players }, Rewrite::Multiply(n)) if n > 1 => {
                if !reads(&players) {
                    return Err(Unreplaced::Players);
                }
                mask = mask | ReplacementKind::Tokens;
            }
            (Replaced::Tokens { players }, Rewrite::Plus(n)) if n > 0 => {
                if !reads(&players) {
                    return Err(Unreplaced::Players);
                }
                mask = mask | ReplacementKind::Tokens;
            }
            (
                Replaced::None
                | Replaced::ThisEnters
                | Replaced::Draw(..)
                | Replaced::PutInto(..)
                | Replaced::Enters { .. }
                | Replaced::BeginTurn
                | Replaced::Damage
                | Replaced::Tokens { .. },
                Rewrite::None
                | Rewrite::ToZone(_)
                | Rewrite::EntersTapped
                | Rewrite::EntersPrepared
                | Rewrite::EntersTappedUnlessFewLands(_)
                | Rewrite::EntersTappedUnlessYouControl(..)
                | Rewrite::EntersTappedUnlessPayLife(_)
                | Rewrite::EntersWithCounters(..)
                | Rewrite::MaySkipTurnToUntap
                | Rewrite::Instead
                | Rewrite::Dredge(_)
                | Rewrite::Choose(_)
                | Rewrite::Multiply(_)
                | Rewrite::Plus(_)
                | Rewrite::EnterAsCopy { .. },
            ) => return Err(Unreplaced::Rewrite),
        }
    }
    Ok(mask)
}
