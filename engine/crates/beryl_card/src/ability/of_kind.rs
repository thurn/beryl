//! What only one kind of ability has, in one field of [`Ability`]: a triggered ability's
//! trigger, a static ability's continuous effect, a replacement ability's replacement, or a
//! mana ability's terms. The builders and readers of each are here.

use super::{Ability, AbilityKind};
use crate::continuous::{LayerSpec, ManaChange, Replacement};
use crate::filter::Filter;
use crate::ids::{Zone, ZoneSet};
use crate::mana::ManaRestriction;
use crate::trigger::Trigger;

/// A condition on a starting deck: "each [card `each` accepts] in your starting deck [passes
/// `has`]". Each card is read as it is outside the game, a split card with both halves'
/// characteristics, and X in a mana cost counts as 0.
#[derive(Clone, Copy, Debug)]
pub struct DeckCondition {
    pub each: Filter,
    pub has: Filter,
}

/// What only one kind of ability has ([`Ability`]'s `of_kind`).
#[derive(Clone, Copy, Debug)]
pub(super) enum OfKind {
    None,
    /// Triggered abilities: what they trigger on.
    Trigger(&'static Trigger),
    /// Static abilities: the continuous effect they generate on the battlefield.
    Layer(&'static LayerSpec),
    /// Static abilities: permission to cast this card while searching its owner's library.
    CastWhileSearching,
    /// Companion, a keyword ability that works outside the game: what every card of its
    /// owner's starting deck must be ([`Ability::companion`]).
    Companion(&'static DeckCondition),
    /// Static abilities: once during each of its controller's turns, permission to cast a
    /// spell the filter accepts from their graveyard ([`Ability::cast_from_graveyard_each_turn`]).
    CastFromGraveyard(&'static Filter),
    /// Static abilities: "bands with other [creatures `filter` accepts]"
    /// ([`Ability::bands_with_other`]).
    BandsWithOther(&'static Filter),
    /// Replacement abilities: what they replace and how.
    Replacement(&'static Replacement),
    /// Permanent mana-changing abilities ([`Ability::mana_change`]).
    ManaChange(&'static ManaChange),
    /// Mana abilities. First, "Activate only if you control a [permanent this filter
    /// accepts]" ([CR 602.5]): "A player can't begin to activate an ability that's
    /// prohibited from being activated." Second, what the mana it makes may be spent on
    /// ([`Ability::spent_only`]).
    ///
    /// [CR 602.5]: https://yawgatog.com/resources/magic-rules/#R6025
    Mana(Option<&'static Filter>, Option<&'static ManaRestriction>),
}

impl OfKind {
    /// The control and spending restrictions of a mana ability.
    const fn mana(self) -> (Option<&'static Filter>, Option<&'static ManaRestriction>) {
        match self {
            OfKind::Mana(filter, spend) => (filter, spend),
            OfKind::None
            | OfKind::Trigger(_)
            | OfKind::Layer(_)
            | OfKind::Replacement(_)
            | OfKind::ManaChange(_)
            | OfKind::CastWhileSearching
            | OfKind::Companion(_)
            | OfKind::CastFromGraveyard(_)
            | OfKind::BandsWithOther(_) => (None, None),
        }
    }
}

impl Ability {
    /// "While you're searching your library, you may cast this card from your library."
    /// This static ability works in the library ([CR 113.6b]): "An ability that states
    /// which zones it functions in functions only from those zones." It allows only the
    /// owner searching their own library to cast this card, before finding a card. The cast
    /// card leaves the library. The search continues with the remaining cards in their
    /// existing order until the effect shuffles.
    ///
    /// [CR 113.6b]: https://yawgatog.com/resources/magic-rules/#R1136b
    pub const fn cast_while_searching() -> Ability {
        Ability {
            of_kind: OfKind::CastWhileSearching,
            zones: ZoneSet::of(Zone::Library),
            ..Ability::of(AbilityKind::Static)
        }
    }

    /// "Companion—Each [card `condition.each` accepts] in your starting deck [passes
    /// `condition.has`]" ([CR 702.139a]): "Companion is a keyword ability that functions
    /// outside the game. It’s written as “Companion—[Condition].” Before the game begins, you
    /// may reveal one card you own from outside the game with a companion ability whose
    /// condition is fulfilled by your starting deck." The ability works in no zone: the rules
    /// read it of the cards outside the game before the game begins.
    ///
    /// [CR 702.139a]: https://yawgatog.com/resources/magic-rules/#R702139a
    pub const fn companion(condition: &'static DeckCondition) -> Ability {
        Ability {
            of_kind: OfKind::Companion(condition),
            zones: ZoneSet::NONE,
            ..Ability::of(AbilityKind::Static)
        }
    }

    /// The condition of a companion ability ([`Ability::companion`]).
    pub const fn companion_condition(&self) -> Option<&'static DeckCondition> {
        match self.of_kind {
            OfKind::Companion(condition) => Some(condition),
            OfKind::None
            | OfKind::Trigger(_)
            | OfKind::Layer(_)
            | OfKind::CastWhileSearching
            | OfKind::CastFromGraveyard(_)
            | OfKind::BandsWithOther(_)
            | OfKind::Replacement(_)
            | OfKind::ManaChange(_)
            | OfKind::Mana(..) => None,
        }
    }

    /// "Once during each of your turns, you may cast a [spell `spells` accepts] from your
    /// graveyard." A static ability of a permanent: its controller may cast a card they own
    /// from their graveyard, with the usual timing and costs, once in each of their turns for
    /// each permanent with the ability. The filter is read of the spell as it would be cast,
    /// with the value of X chosen. A spell cast from the graveyard under another permission,
    /// such as flashback, leaves the permission unused.
    pub const fn cast_from_graveyard_each_turn(spells: &'static Filter) -> Ability {
        Ability {
            of_kind: OfKind::CastFromGraveyard(spells),
            ..Ability::of(AbilityKind::Static)
        }
    }

    /// The spells a permission to cast from a graveyard once each turn accepts
    /// ([`Ability::cast_from_graveyard_each_turn`]).
    pub const fn graveyard_cast(&self) -> Option<&'static Filter> {
        match self.of_kind {
            OfKind::CastFromGraveyard(spells) => Some(spells),
            OfKind::BandsWithOther(_)
            | OfKind::None
            | OfKind::Trigger(_)
            | OfKind::Layer(_)
            | OfKind::CastWhileSearching
            | OfKind::Companion(_)
            | OfKind::Replacement(_)
            | OfKind::ManaChange(_)
            | OfKind::Mana(..) => None,
        }
    }

    /// "Bands with other [creatures `quality` accepts]" ([CR 702.22c]): "They may also declare
    /// that one or more attacking [quality] creatures with “bands with other [quality]” and any
    /// number of other attacking [quality] creatures are all in a band." The same quality
    /// decides who assigns combat damage ([CR 702.22j]): an attacker blocked by a creature
    /// that has this ability and passes `quality` and by another creature that passes it has
    /// its damage assigned by the defending player, and [CR 702.22k] says the same of a
    /// blocker for the active player.
    ///
    /// It is a form of banding ([CR 702.22b]): "If an effect causes a permanent to lose
    /// banding, the permanent loses all “bands with other” abilities as well." It is an
    /// ability of its own, not a keyword bit, so that a face can print it, grant it
    /// ([`crate::Modification::Grant`]) and lose it with its other abilities in layer 6.
    ///
    /// [CR 702.22b]: https://yawgatog.com/resources/magic-rules/#R70222b
    /// [CR 702.22c]: https://yawgatog.com/resources/magic-rules/#R70222c
    /// [CR 702.22j]: https://yawgatog.com/resources/magic-rules/#R70222j
    /// [CR 702.22k]: https://yawgatog.com/resources/magic-rules/#R70222k
    pub const fn bands_with_other(quality: &'static Filter) -> Ability {
        Ability {
            of_kind: OfKind::BandsWithOther(quality),
            ..Ability::of(AbilityKind::Static)
        }
    }

    /// The quality of a "bands with other" ability ([`Ability::bands_with_other`]).
    pub const fn bands_with(&self) -> Option<&'static Filter> {
        match self.of_kind {
            OfKind::BandsWithOther(quality) => Some(quality),
            OfKind::None
            | OfKind::Trigger(_)
            | OfKind::Layer(_)
            | OfKind::CastWhileSearching
            | OfKind::Companion(_)
            | OfKind::CastFromGraveyard(_)
            | OfKind::Replacement(_)
            | OfKind::ManaChange(_)
            | OfKind::Mana(..) => None,
        }
    }

    /// Whether this ability permits casting this card from its owner's library while they search.
    pub const fn permits_cast_while_searching(&self) -> bool {
        matches!(self.of_kind, OfKind::CastWhileSearching) && self.zones.contains(Zone::Library)
    }

    /// "Activate only if you control a [permanent `filter` accepts]" (CR 602.5), of a mana
    /// ability.
    #[must_use]
    pub const fn if_you_control(self, filter: &'static Filter) -> Ability {
        assert!(
            matches!(self.kind, AbilityKind::Mana),
            "only a mana ability is restricted by what you control"
        );
        let spend = self.spend();
        Ability {
            of_kind: OfKind::Mana(Some(filter), spend),
            ..self
        }
    }

    /// "Spend this mana only to [`restriction`'s spells], and [its rider]" ([CR 106.6]): "Some
    /// spells or abilities that produce mana restrict how that mana can be spent, have an
    /// additional effect that affects the spell or ability that mana is spent on", of a mana
    /// ability.
    ///
    /// [CR 106.6]: https://yawgatog.com/resources/magic-rules/#R1066
    #[must_use]
    pub const fn spent_only(self, restriction: &'static ManaRestriction) -> Ability {
        assert!(
            matches!(self.kind, AbilityKind::Mana),
            "only a mana ability's mana is restricted"
        );
        let only_if_you_control = self.only_if_you_control();
        Ability {
            of_kind: OfKind::Mana(only_if_you_control, Some(restriction)),
            ..self
        }
    }

    /// Changes what a triggered ability listens to, keeping its effect and restrictions.
    /// Used when comparing a definition to one with a different trigger.
    ///
    /// # Panics
    ///
    /// If this is not a triggered ability.
    #[must_use]
    pub const fn with_trigger(self, trigger: &'static Trigger) -> Self {
        assert!(matches!(self.kind, AbilityKind::Triggered));
        Self {
            of_kind: OfKind::Trigger(trigger),
            ..self
        }
    }

    /// A triggered ability: what it triggers on.
    #[inline]
    pub const fn trigger(&self) -> Option<&'static Trigger> {
        match self.of_kind {
            OfKind::Trigger(trigger) => Some(trigger),
            OfKind::None
            | OfKind::Layer(_)
            | OfKind::Replacement(_)
            | OfKind::ManaChange(_)
            | OfKind::Mana(..)
            | OfKind::CastWhileSearching
            | OfKind::Companion(_)
            | OfKind::CastFromGraveyard(_)
            | OfKind::BandsWithOther(_) => None,
        }
    }

    /// A replacement ability: what it replaces and how; `None` for any other kind.
    #[inline]
    pub const fn replacement(&self) -> Option<&'static Replacement> {
        match self.of_kind {
            OfKind::Replacement(replacement) => Some(replacement),
            OfKind::None
            | OfKind::Trigger(_)
            | OfKind::Layer(_)
            | OfKind::ManaChange(_)
            | OfKind::Mana(..)
            | OfKind::CastWhileSearching
            | OfKind::Companion(_)
            | OfKind::CastFromGraveyard(_)
            | OfKind::BandsWithOther(_) => None,
        }
    }

    /// A permanent's mana-changing ability ([`Ability::mana_change`]); `None` for any other kind.
    #[inline]
    pub const fn changes_mana(&self) -> Option<&'static ManaChange> {
        match self.of_kind {
            OfKind::ManaChange(change) => Some(change),
            OfKind::None
            | OfKind::Trigger(_)
            | OfKind::Layer(_)
            | OfKind::Replacement(_)
            | OfKind::Mana(..)
            | OfKind::CastWhileSearching
            | OfKind::Companion(_)
            | OfKind::CastFromGraveyard(_)
            | OfKind::BandsWithOther(_) => None,
        }
    }

    /// A static ability: the continuous effect it generates.
    #[inline]
    pub const fn layer(&self) -> Option<&'static LayerSpec> {
        match self.of_kind {
            OfKind::Layer(layer) => Some(layer),
            OfKind::None
            | OfKind::Trigger(_)
            | OfKind::Replacement(_)
            | OfKind::ManaChange(_)
            | OfKind::Mana(..)
            | OfKind::CastWhileSearching
            | OfKind::Companion(_)
            | OfKind::CastFromGraveyard(_)
            | OfKind::BandsWithOther(_) => None,
        }
    }

    /// A mana ability: what its controller must control to activate it
    /// ([`Ability::if_you_control`]).
    #[inline]
    pub const fn only_if_you_control(&self) -> Option<&'static Filter> {
        self.of_kind.mana().0
    }

    /// A mana ability: what the mana it makes may be spent on, and what it does to the spell
    /// it pays for ([`Ability::spent_only`]).
    #[inline]
    pub const fn spend(&self) -> Option<&'static ManaRestriction> {
        self.of_kind.mana().1
    }
}
