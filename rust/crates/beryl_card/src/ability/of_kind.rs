//! What only one kind of ability has, in one field of [`Ability`]: a triggered ability's
//! trigger, a static ability's continuous effect, a replacement ability's replacement, or a
//! mana ability's terms. The builders and readers of each are here.

use super::{Ability, AbilityKind};
use crate::continuous::{LayerSpec, Replacement};
use crate::filter::Filter;
use crate::mana::ManaRestriction;
use crate::trigger::Trigger;

/// What only one kind of ability has ([`Ability`]'s `of_kind`).
#[derive(Clone, Copy, Debug)]
pub(super) enum OfKind {
    None,
    /// Triggered abilities: what they trigger on.
    Trigger(&'static Trigger),
    /// Static abilities: the continuous effect they generate on the battlefield.
    Layer(&'static LayerSpec),
    /// Replacement abilities: what they replace and how.
    Replacement(&'static Replacement),
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
            OfKind::None | OfKind::Trigger(_) | OfKind::Layer(_) | OfKind::Replacement(_) => {
                (None, None)
            }
        }
    }
}

impl Ability {
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

    /// A triggered ability: what it triggers on.
    #[inline]
    pub const fn trigger(&self) -> Option<&'static Trigger> {
        match self.of_kind {
            OfKind::Trigger(trigger) => Some(trigger),
            OfKind::None | OfKind::Layer(_) | OfKind::Replacement(_) | OfKind::Mana(..) => None,
        }
    }

    /// A replacement ability: what it replaces and how; `None` for any other kind.
    #[inline]
    pub const fn replacement(&self) -> Option<&'static Replacement> {
        match self.of_kind {
            OfKind::Replacement(replacement) => Some(replacement),
            OfKind::None | OfKind::Trigger(_) | OfKind::Layer(_) | OfKind::Mana(..) => None,
        }
    }

    /// A static ability: the continuous effect it generates.
    #[inline]
    pub const fn layer(&self) -> Option<&'static LayerSpec> {
        match self.of_kind {
            OfKind::Layer(layer) => Some(layer),
            OfKind::None | OfKind::Trigger(_) | OfKind::Replacement(_) | OfKind::Mana(..) => None,
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
