//! `Filter`: the predicates targets and affected sets are written in.

use crate::characteristics::{Colors, Subtype, Supertypes, Types};
use crate::effect::Players;

/// A predicate over objects. Evaluating one is a pass over a candidate set; the hot ones
/// become ANDs over per-slot attribute bitboards as the cards that need them arrive.
///
/// A permanent's characteristics and controller are its derived ones, after continuous
/// effects; elsewhere they are the face's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    Any,
    /// Has at least one of these card types.
    Types(Types),
    Supertypes(Supertypes),
    Subtype(Subtype),
    /// Is at least one of these colors: "a blue card".
    Colors(Colors),
    /// Controlled by (on the battlefield or stack) or owned by (elsewhere) one of these
    /// players, read for the controller of the spell or ability the filter is part of: "you
    /// control", "an opponent controls", "your graveyard"
    /// ([CR 109.4](https://yawgatog.com/resources/magic-rules/#R1094)): "Only objects on the
    /// stack or on the battlefield have a controller." Held by reference, as a [`Players`]
    /// names objects and so filters.
    ///
    /// The players are the card's words, not what they come to in a two-player game.
    /// "You don't control" is `Filter::Not(&Filter::Whose(&Players::You))`, which a
    /// teammate's object passes and [`Players::EachOpponent`] does not
    /// ([CR 102.3](https://yawgatog.com/resources/magic-rules/#R1023)): "a player's
    /// teammates are the other players on their team, and the player's opponents are all
    /// players not on their team."
    Whose(&'static Players),
    Tapped,
    ManaValueAtMost(u8),
    /// Mana value X or less, X being the value of the spell or ability whose target this is
    /// (Thieving Skydiver's "target artifact with mana value X or less").
    ManaValueAtMostX,
    Not(&'static Filter),
    All(&'static [Filter]),
    AnyOf(&'static [Filter]),
}

impl Filter {
    /// Whether this means something only a resolving spell or ability has
    /// ([`crate::Objects::needs_a_resolution`]): its X, or players that do.
    pub const fn needs_a_resolution(&self) -> bool {
        match self {
            Filter::Any
            | Filter::Types(_)
            | Filter::Supertypes(_)
            | Filter::Subtype(_)
            | Filter::Colors(_)
            | Filter::Tapped
            | Filter::ManaValueAtMost(_) => false,
            Filter::ManaValueAtMostX => true,
            Filter::Whose(players) => players.needs_a_resolution(),
            Filter::Not(filter) => filter.needs_a_resolution(),
            Filter::All(filters) | Filter::AnyOf(filters) => {
                let mut i = 0;
                while i < filters.len() {
                    if filters[i].needs_a_resolution() {
                        return true;
                    }
                    i += 1;
                }
                false
            }
        }
    }
}
