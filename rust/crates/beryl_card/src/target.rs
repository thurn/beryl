//! Targets: what one instance of the word "target" may choose.

use crate::characteristics::Types;
use crate::effect::Players;
use crate::filter::Filter;

/// What kind of thing a target is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TargetKind {
    /// A permanent: an object on the battlefield.
    Permanent,
    /// A spell on the stack.
    Spell,
    /// A spell on the stack or a permanent, such as "target spell or nonland permanent".
    SpellOrPermanent,
    /// A card in a graveyard, such as "target instant or sorcery card from your graveyard".
    GraveyardCard,
    /// A player. The filter is [`Filter::Any`] for "target player" or [`Filter::Whose`], read
    /// for players, for "target opponent".
    Player,
    /// A permanent or a player, such as "any target". The filter applies to the permanents
    /// only: every player may be chosen.
    PermanentOrPlayer,
}

/// How an effect takes what it targets from its controller, which a player is told as they
/// choose the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Takes {
    /// A permanent, from the battlefield: destroyed, exiled, sacrificed, returned to a hand
    /// or put into a library.
    Removal,
    /// A spell, from the stack
    /// ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a)): "To counter a spell
    /// or ability means to cancel it, removing it from the stack. It doesn't resolve and none
    /// of its effects occur."
    Counter,
}

/// One instance of the word "target" in a spell or ability: what it may choose and how many
/// ([CR 115.1](https://yawgatog.com/resources/magic-rules/#R1151)).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TargetSpec {
    pub kind: TargetKind,
    pub filter: Filter,
    /// Exactly this many, or up to this many when `optional`
    /// ([CR 115.6](https://yawgatog.com/resources/magic-rules/#R1156)): "A spell or ability
    /// that requires targets may allow zero targets to be chosen."
    pub count: u8,
    pub optional: bool,
    /// "Other target": not the object whose ability this is.
    pub other: bool,
}

impl TargetSpec {
    /// One target of a kind that passes a filter.
    pub const fn one(kind: TargetKind, filter: Filter) -> TargetSpec {
        TargetSpec {
            kind,
            filter,
            count: 1,
            optional: false,
            other: false,
        }
    }

    /// "Up to one target".
    pub const fn up_to_one(kind: TargetKind, filter: Filter) -> TargetSpec {
        TargetSpec {
            optional: true,
            ..TargetSpec::one(kind, filter)
        }
    }

    /// "Target spell".
    pub const fn spell() -> TargetSpec {
        TargetSpec::one(TargetKind::Spell, Filter::Any)
    }

    /// "Target player".
    pub const fn player() -> TargetSpec {
        TargetSpec::one(TargetKind::Player, Filter::Any)
    }

    /// "Target opponent".
    pub const fn opponent() -> TargetSpec {
        TargetSpec::one(TargetKind::Player, Filter::Whose(&Players::EachOpponent))
    }

    /// "Any target" ([CR 115.4](https://yawgatog.com/resources/magic-rules/#R1154)): "These
    /// targets may be creatures, players, planeswalkers, or battles."
    pub const fn any_target() -> TargetSpec {
        TargetSpec::one(
            TargetKind::PermanentOrPlayer,
            Filter::Types(
                Types::CREATURE
                    .union(Types::PLANESWALKER)
                    .union(Types::BATTLE),
            ),
        )
    }

    /// "Target creature".
    pub const fn creature() -> TargetSpec {
        TargetSpec::permanent(Filter::Types(Types::CREATURE))
    }

    /// "Target [permanent that passes `filter`]".
    pub const fn permanent(filter: Filter) -> TargetSpec {
        TargetSpec::one(TargetKind::Permanent, filter)
    }

    /// "Target [card that passes `filter`] from a graveyard".
    pub const fn graveyard_card(filter: Filter) -> TargetSpec {
        TargetSpec::one(TargetKind::GraveyardCard, filter)
    }

    /// The same spec for "up to one target": zero targets may be chosen.
    #[must_use]
    pub const fn optional(self) -> TargetSpec {
        TargetSpec {
            optional: true,
            ..self
        }
    }

    /// The same spec for "another target": its ability's source can't be chosen.
    #[must_use]
    pub const fn other(self) -> TargetSpec {
        TargetSpec {
            other: true,
            ..self
        }
    }
}
