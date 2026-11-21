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
    /// An activated or triggered ability on the stack: "target activated or triggered
    /// ability". The filter of this and the next two is [`Filter::Any`]. A mana ability is
    /// never on the stack, so never a target ([CR 605.3b]): "An activated mana ability doesn't
    /// go on the stack, so it can't be targeted, countered, or otherwise responded to."
    ///
    /// [CR 605.3b]: https://yawgatog.com/resources/magic-rules/#R6053b
    Ability,
    /// An activated ability on the stack: "target activated ability".
    ActivatedAbility,
    /// A triggered ability on the stack: "target triggered ability".
    TriggeredAbility,
}

impl TargetKind {
    /// The kinds of abilities on the stack it may be, if it is an ability: a kind without a
    /// payload, so that a spec stays one byte for the rules that compare kinds.
    pub const fn abilities(self) -> Option<Abilities> {
        match self {
            TargetKind::Ability => Some(Abilities::ActivatedOrTriggered),
            TargetKind::ActivatedAbility => Some(Abilities::Activated),
            TargetKind::TriggeredAbility => Some(Abilities::Triggered),
            TargetKind::Permanent
            | TargetKind::Spell
            | TargetKind::SpellOrPermanent
            | TargetKind::GraveyardCard
            | TargetKind::Player
            | TargetKind::PermanentOrPlayer => None,
        }
    }
}

/// The kinds of abilities on the stack a target may be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Abilities {
    ActivatedOrTriggered,
    Activated,
    Triggered,
}

impl Abilities {
    /// Whether an ability on the stack, activated or else triggered, is of these kinds.
    pub const fn include(self, triggered: bool) -> bool {
        match self {
            Abilities::ActivatedOrTriggered => true,
            Abilities::Activated => !triggered,
            Abilities::Triggered => triggered,
        }
    }

    /// The noun that names them: "activated or triggered ability".
    pub const fn noun(self) -> &'static str {
        match self {
            Abilities::ActivatedOrTriggered => "activated or triggered ability",
            Abilities::Activated => "activated ability",
            Abilities::Triggered => "triggered ability",
        }
    }
}

/// How an effect takes what it targets from its controller, which a player is told as they
/// choose the target.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Takes {
    /// A permanent, from the battlefield: destroyed, exiled, sacrificed, returned to a hand
    /// or put into a library.
    Removal,
    /// A spell or an ability, from the stack
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
    /// Only a permanent an ability of the object with this spec put onto the battlefield,
    /// as Animate Dead's "enchant creature put onto the battlefield with this Aura" says.
    pub put_with_this: bool,
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
            put_with_this: false,
        }
    }

    /// "Up to one target".
    pub const fn up_to_one(kind: TargetKind, filter: Filter) -> TargetSpec {
        TargetSpec {
            optional: true,
            ..TargetSpec::one(kind, filter)
        }
    }

    /// "Up to [count] target": zero to `count` targets may be chosen.
    pub const fn up_to(count: u8, kind: TargetKind, filter: Filter) -> TargetSpec {
        TargetSpec {
            count,
            ..TargetSpec::up_to_one(kind, filter)
        }
    }

    /// "Target spell".
    pub const fn spell() -> TargetSpec {
        TargetSpec::one(TargetKind::Spell, Filter::Any)
    }

    /// "Target activated or triggered ability".
    pub const fn ability() -> TargetSpec {
        TargetSpec::one(TargetKind::Ability, Filter::Any)
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

    /// The same spec for "[permanent] put onto the battlefield with this": only one an
    /// ability of the object with the spec put there
    /// ([`crate::effect::PutOnto::linked`]).
    #[must_use]
    pub const fn put_with_this(self) -> TargetSpec {
        TargetSpec {
            put_with_this: true,
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
