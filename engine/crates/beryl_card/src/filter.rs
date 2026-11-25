//! `Filter`: the predicates targets and affected sets are written in.

use crate::characteristics::{Colors, Subtype, Supertypes, Types};
use crate::effect::Players;
use crate::ids::Zone;
use crate::words::{ColorWords, SubtypeWord};

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
    /// Has this subtype: "Aura", "a Mountain". The word is read through the text changes of
    /// the object the filter is printed on ([`crate::words`]); [`Filter::Subtype`] builds it.
    SubtypeWord(SubtypeWord),
    /// Is at least one of these colors: "a blue card". The words are read as
    /// [`Filter::SubtypeWord`]'s is; [`Filter::Colors`] builds it.
    ColorWords(ColorWords),
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
    /// Owned by one of these players, wherever it is, read as [`Filter::Whose`] is: "creature
    /// you own" ([CR 108.3]): "The owner of a card in the game is the player who started the
    /// game with it in their deck."
    ///
    /// [CR 108.3]: https://yawgatog.com/resources/magic-rules/#R1083
    Owned(&'static Players),
    Tapped,
    /// Was declared as an attacker this turn ([CR 508.4]).
    ///
    /// [CR 508.4]: https://yawgatog.com/resources/magic-rules/#R5084
    AttackedThisTurn,
    /// Is a token ([CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111)): "Some
    /// effects put tokens onto the battlefield." Not a token is "nontoken".
    Token,
    /// Has this name: "creatures named Wolves of the Hunt"
    /// ([CR 201.2a](https://yawgatog.com/resources/magic-rules/#R2012a)): "Two or more objects
    /// have the same name if they have at least one name in common". A name is a copiable
    /// value ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)), so a copy of a
    /// permanent with the name has it too. A face-down permanent has no name and never passes.
    Named(&'static str),
    ManaValueAtMost(u8),
    /// Power N or less: "creature an opponent controls with power 3 or less". A permanent's
    /// power is its derived one, after continuous effects
    /// ([CR 613.4](https://yawgatog.com/resources/magic-rules/#R6134)); elsewhere it is the
    /// face's.
    PowerAtMost(i8),
    /// Mana value X or less, X being the value of the spell or ability whose target this is
    /// (Thieving Skydiver's "target artifact with mana value X or less").
    ManaValueAtMostX,
    /// Mana value less than or equal to the power of the object the filter is printed on,
    /// as Arcane Proxy's "instant or sorcery card with mana value less than or equal to this
    /// creature's power". The power is read whenever the filter is, so a target that the
    /// creature's power has fallen below is illegal as the ability resolves (see
    /// [CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)); once the creature has
    /// left the battlefield, its last known power is used
    /// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)). Only a target's
    /// filter says it, and a filter has this or [`Filter::ManaValueAtMostX`], never both: the
    /// target rules read the power where they would read X.
    ManaValueAtMostPower,
    Not(&'static Filter),
    All(&'static [Filter]),
    AnyOf(&'static [Filter]),
    /// Is in this zone: "in your graveyard", with [`Filter::Whose`] for whose. A filter that
    /// only names objects in a zone needs none, so only an effect that reaches every zone
    /// ([`crate::Objects::Everywhere`]) says one.
    InZone(Zone),
}

impl Filter {
    /// Whether this compares with the power of the object it is printed on
    /// ([`Filter::ManaValueAtMostPower`]), itself or through a filter it is made of.
    pub const fn reads_source_power(&self) -> bool {
        match self {
            Filter::ManaValueAtMostPower => true,
            Filter::Not(filter) => filter.reads_source_power(),
            Filter::All(filters) | Filter::AnyOf(filters) => {
                let mut i = 0;
                while i < filters.len() {
                    if filters[i].reads_source_power() {
                        return true;
                    }
                    i += 1;
                }
                false
            }
            Filter::Any
            | Filter::Types(_)
            | Filter::Supertypes(_)
            | Filter::SubtypeWord(_)
            | Filter::ColorWords(_)
            | Filter::Whose(_)
            | Filter::Owned(_)
            | Filter::Tapped
            | Filter::AttackedThisTurn
            | Filter::Token
            | Filter::Named(_)
            | Filter::ManaValueAtMost(_)
            | Filter::PowerAtMost(_)
            | Filter::ManaValueAtMostX
            | Filter::InZone(_) => false,
        }
    }

    /// The filters required together, when this is a conjunction.
    pub(crate) const fn conjunction(self) -> Option<&'static [Filter]> {
        match self {
            Filter::All(filters) => Some(filters),
            Filter::Any
            | Filter::Types(_)
            | Filter::Supertypes(_)
            | Filter::SubtypeWord(_)
            | Filter::ColorWords(_)
            | Filter::Whose(_)
            | Filter::Owned(_)
            | Filter::AttackedThisTurn
            | Filter::Tapped
            | Filter::Token
            | Filter::Named(_)
            | Filter::ManaValueAtMost(_)
            | Filter::PowerAtMost(_)
            | Filter::ManaValueAtMostX
            | Filter::ManaValueAtMostPower
            | Filter::Not(_)
            | Filter::AnyOf(_)
            | Filter::InZone(_) => None,
        }
    }

    /// Has `subtype`, as the card's text prints it.
    #[allow(non_snake_case)]
    #[inline]
    pub const fn Subtype(subtype: Subtype) -> Filter {
        Filter::SubtypeWord(SubtypeWord::new(subtype))
    }

    /// Is at least one of `colors`, as the card's text prints them.
    #[allow(non_snake_case)]
    #[inline]
    pub const fn Colors(colors: Colors) -> Filter {
        Filter::ColorWords(ColorWords::new(colors))
    }

    /// Whether this means something only a resolving spell or ability has
    /// ([`crate::Objects::needs_a_resolution`]): its X, or players that do.
    pub const fn needs_a_resolution(&self) -> bool {
        match self {
            Filter::Any
            | Filter::Types(_)
            | Filter::Supertypes(_)
            | Filter::SubtypeWord(_)
            | Filter::ColorWords(_)
            | Filter::AttackedThisTurn
            | Filter::Tapped
            | Filter::Token
            | Filter::Named(_)
            | Filter::ManaValueAtMost(_)
            | Filter::PowerAtMost(_)
            | Filter::ManaValueAtMostPower
            | Filter::InZone(_) => false,
            Filter::ManaValueAtMostX => true,
            Filter::Whose(players) | Filter::Owned(players) => players.needs_a_resolution(),
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
