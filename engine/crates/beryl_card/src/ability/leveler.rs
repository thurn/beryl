//! Level up and the level symbols of leveler cards.

use super::Ability;
use crate::continuous::{LayerSpec, Modification};
use crate::cost::Cost;
use crate::counter::CounterKind;
use crate::effect::{Amount, Comparison, Condition, Effect, Objects};

/// What level up's ability does: "Put a level counter on this permanent."
pub(crate) static LEVEL_UP: Effect = Effect::AddCounters {
    objects: Objects::This,
    kind: CounterKind::Level,
    count: Amount::Fixed(1),
};

/// The number of level counters on the permanent with the ability.
const LEVEL: Amount = Amount::Counters(Objects::This, CounterKind::Level);

/// The condition of a level symbol from `FROM` to `TO` level counters, both included;
/// `TO` is `u8::MAX` for a symbol with no upper end.
struct Band<const FROM: u8, const TO: u8>;

impl<const FROM: u8, const TO: u8> Band<FROM, TO> {
    const CONDITION: Condition = if TO == u8::MAX {
        Condition::Compare(LEVEL, Comparison::AtLeast, Amount::Fixed(FROM))
    } else {
        Condition::All(&[
            Condition::Compare(LEVEL, Comparison::AtLeast, Amount::Fixed(FROM)),
            Condition::Compare(LEVEL, Comparison::AtMost, Amount::Fixed(TO)),
        ])
    };
}

impl Ability {
    /// "Level up [cost]" ([CR 702.87a]): "Level up is an activated ability. “Level up
    /// [cost]” means “[Cost]: Put a level counter on this permanent. Activate only as a
    /// sorcery.”"
    ///
    /// [CR 702.87a]: https://yawgatog.com/resources/magic-rules/#R70287a
    pub const fn level_up(cost: Cost) -> Ability {
        Ability::activated(cost).as_a_sorcery().effect(&LEVEL_UP)
    }

    /// Whether this is a level up ability ([`Ability::level_up`]).
    pub fn is_level_up(&self) -> bool {
        matches!(
            self.body(),
            super::Behavior::Data(Effect::AddCounters {
                objects: Objects::This,
                kind: CounterKind::Level,
                count: Amount::Fixed(1),
            })
        ) && self.timing() == super::Timing::Sorcery
            && self.kind() == super::AbilityKind::Activated
    }
}

impl LayerSpec {
    /// A level symbol "LEVEL FROM-TO" followed by its base power and toughness and its
    /// abilities ([CR 711.2a]): "“{LEVEL N1-N2} [Abilities] [P/T]” means “As long as this
    /// creature has at least N1 level counters on it, but no more than N2 level counters on
    /// it, it has base power and toughness [P/T] and has [abilities].”" The modifications
    /// are a [`Modification::SetBasePt`] of fixed amounts, then the keywords it adds and the
    /// abilities it grants. Its effect has the leveler's own timestamp, however recently a
    /// level counter was put on it, as any static ability of a permanent does.
    ///
    /// # Panics
    ///
    /// As the definition compiles, if `TO` is below `FROM` or is `u8::MAX`.
    ///
    /// [CR 711.2a]: https://yawgatog.com/resources/magic-rules/#R7112a
    pub const fn leveled<const FROM: u8, const TO: u8>(
        modifications: &'static [Modification],
    ) -> LayerSpec {
        assert!(FROM <= TO && TO < u8::MAX);
        Self::level_symbol(Band::<FROM, TO>::CONDITION, modifications)
    }

    /// A level symbol "LEVEL FROM+" followed by its base power and toughness and its
    /// abilities ([CR 711.2b]): "“{LEVEL N3+} [Abilities] [P/T]” means “As long as this
    /// creature has N3 or more level counters on it, it has base power and toughness [P/T]
    /// and has [abilities].”" Its modifications are as [`LayerSpec::leveled`]'s.
    ///
    /// [CR 711.2b]: https://yawgatog.com/resources/magic-rules/#R7112b
    pub const fn leveled_from<const FROM: u8>(modifications: &'static [Modification]) -> LayerSpec {
        Self::level_symbol(Band::<FROM, { u8::MAX }>::CONDITION, modifications)
    }

    const fn level_symbol(
        condition: Condition,
        modifications: &'static [Modification],
    ) -> LayerSpec {
        assert!(
            matches!(
                modifications.first(),
                Some(Modification::SetBasePt(Amount::Fixed(_), Amount::Fixed(_)))
            ),
            "a level symbol sets a fixed base power and toughness first"
        );
        LayerSpec {
            affected: Objects::This,
            condition: Some(condition),
            modifications,
        }
    }

    /// The range of level counters this holds for, if it is a level symbol
    /// ([`LayerSpec::leveled`] or [`LayerSpec::leveled_from`]): the least number and the
    /// greatest, or `None` for a symbol with no upper end.
    pub fn level_band(&self) -> Option<(u8, Option<u8>)> {
        let at_least = |condition: &Condition| {
            if let Condition::Compare(
                Amount::Counters(Objects::This, CounterKind::Level),
                Comparison::AtLeast,
                Amount::Fixed(n),
            ) = *condition
            {
                Some(n)
            } else {
                None
            }
        };
        if !matches!(self.affected, Objects::This)
            || !matches!(
                self.modifications.first(),
                Some(Modification::SetBasePt(Amount::Fixed(_), Amount::Fixed(_)))
            )
        {
            return None;
        }
        let condition = self.condition.as_ref()?;
        if let Condition::All(
            [
                from,
                Condition::Compare(
                    Amount::Counters(Objects::This, CounterKind::Level),
                    Comparison::AtMost,
                    Amount::Fixed(to),
                ),
            ],
        ) = condition
        {
            return Some((at_least(from)?, Some(*to)));
        }
        Some((at_least(condition)?, None))
    }
}
