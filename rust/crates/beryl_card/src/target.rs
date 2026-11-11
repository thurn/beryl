//! Targets: what one instance of the word "target" may choose.

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
}

/// One instance of the word "target" in a spell or ability: what it may choose and how many
/// ([CR 115.1](https://yawgatog.com/resources/magic-rules/#R1151)).
#[derive(Clone, Copy, Debug)]
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
    /// This effect removes a targeted permanent from the battlefield. Presentation metadata
    /// for confirming a player's own target, independent of target legality (CR 115.1).
    pub removes_permanent: bool,
    /// This effect counters a targeted spell
    /// ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a): "To counter a spell
    /// or ability means to cancel it, removing it from the stack"). Presentation metadata, as
    /// `removes_permanent` is.
    pub counters_spell: bool,
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
            removes_permanent: false,
            counters_spell: false,
        }
    }

    /// "Up to one target".
    pub const fn up_to_one(kind: TargetKind, filter: Filter) -> TargetSpec {
        TargetSpec {
            optional: true,
            ..TargetSpec::one(kind, filter)
        }
    }

    /// Marks effects such as destroy, exile, or bounce that remove the target from play.
    #[must_use]
    pub const fn removal(self) -> Self {
        Self {
            removes_permanent: true,
            ..self
        }
    }

    /// Marks effects that counter the target.
    #[must_use]
    pub const fn counter(self) -> Self {
        Self {
            counters_spell: true,
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
