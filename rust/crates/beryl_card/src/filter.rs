//! `Filter`: the predicates targets and affected sets are written in.

use crate::characteristics::{Subtype, Supertypes, Types};

/// Which player a filter or a trigger means, relative to the player it is read for: the
/// controller of the spell or ability it is part of.
///
/// An entry says the card's words, not what they come to in a two-player game (plan K7).
/// "You don't control" is no entry: it is `Filter::Not(&Filter::Whose(Whose::You))`, which a
/// teammate's object passes and [`Whose::AnOpponent`] does not
/// ([CR 102.3](https://yawgatog.com/resources/magic-rules/#R1023)): "a player's
/// teammates are the other players on their team, and the player's opponents are all players
/// not on their team."
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Whose {
    /// "You control", "your graveyard", "your upkeep"
    /// ([CR 109.5](https://yawgatog.com/resources/magic-rules/#R1095)): "The words 'you' and
    /// 'your' on an object refer to the object's controller, its would-be controller (if a
    /// player is attempting to play, cast, or activate it), or its owner (if it has no
    /// controller)."
    You,
    /// "An opponent controls", "target opponent", "each opponent's upkeep": any one of that
    /// player's opponents, whichever it is
    /// ([CR 102.2](https://yawgatog.com/resources/magic-rules/#R1022)): "In a two-player
    /// game, a player's opponent is the other player." With more players, any of them (CR
    /// 102.3).
    AnOpponent,
}

/// A predicate over objects. Evaluating one is a pass over a candidate set; the hot ones
/// become ANDs over per-slot attribute bitboards as the cards that need them arrive.
///
/// A permanent's characteristics and controller are its derived ones, after continuous
/// effects; elsewhere they are the face's.
#[derive(Clone, Copy, Debug)]
pub enum Filter {
    Any,
    /// Has at least one of these card types.
    Types(Types),
    Supertypes(Supertypes),
    Subtype(Subtype),
    /// Controlled by (on the battlefield or stack) or owned by (elsewhere) this player
    /// ([CR 109.4](https://yawgatog.com/resources/magic-rules/#R1094)): "Only objects on the
    /// stack or on the battlefield have a controller."
    Whose(Whose),
    Tapped,
    ManaValueAtMost(u8),
    /// Mana value X or less, X being the value of the spell or ability whose target this is
    /// (Thieving Skydiver's "target artifact with mana value X or less").
    ManaValueAtMostX,
    Not(&'static Filter),
    All(&'static [Filter]),
    AnyOf(&'static [Filter]),
}
