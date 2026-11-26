//! What the mana solver reads of a mana ability: whether it taps for mana alone, sacrifices its
//! source, or has a price, and whether a face has another ability that tapping it would cost.
//!
//! Implements: none. Classifies mana abilities for the mana solver.

use beryl_card::{Ability, AbilityKind, CostAction, CostPart, Face};

/// Whether a face has an ability other than those the solver taps for, which tapping it for
/// mana would cost. Abilities that work only from a hand, such as cycling, don't count.
pub fn has_other_ability(face: &Face) -> bool {
    face.abilities.iter().any(|a| {
        matches!(a.kind(), AbilityKind::Activated | AbilityKind::Mana)
            && !is_simple(a)
            && !a.cost().does(CostAction::Discard)
    })
}

/// Whether an ability is one the solver taps for: a mana ability whose whole cost is {T}, and
/// whose mana may be spent on anything.
#[inline]
pub fn is_simple(a: &Ability) -> bool {
    is_tap_for_mana(a) && a.spend().is_none()
}

/// Whether an ability is a mana ability whose whole cost is {T}: [`is_simple`], or one whose
/// mana is restricted, which the solver taps only for a payment the restriction accepts.
#[inline]
pub fn is_tap_for_mana(a: &Ability) -> bool {
    a.kind() == AbilityKind::Mana
        && a.cost().mana.is_zero()
        && matches!(a.cost().parts(), [CostPart::This(CostAction::Tap)])
}

/// Whether an ability is a priced mana ability: one whose cost has a mana part, as "{1}, {T}:
/// Add {W}{U}." does, or a part that chooses what pays it ([`CostPart::Chosen`]), as
/// "Sacrifice an artifact: Add {C}{C}." does. Paying such a cost may take the mana or the
/// permanents of other sources, so the plain solver, which taps each source once for nothing
/// more, plans none: they are planned in sequence (see [CR 605.3a]): "A player may activate an
/// activated mana ability whenever they have priority, whenever they are casting a spell or
/// activating an ability that requires a mana payment, or whenever a rule or effect asks for a
/// mana payment".
///
/// [CR 605.3a]: https://yawgatog.com/resources/magic-rules/#R6053a
#[inline]
pub fn is_priced(a: &Ability) -> bool {
    a.kind() == AbilityKind::Mana
        && (!a.cost().mana.is_zero()
            || a.cost()
                .parts()
                .iter()
                .any(|part| matches!(part, CostPart::Chosen(..))))
}

/// Whether an ability is a mana ability whose whole cost is {T} and sacrificing its source, as a
/// Treasure's (see [CR 111.10a]): "{T}, Sacrifice this token: Add one mana of any color." Spending the
/// permanent is a resource a plan consumes (§10): the solver taps such a source only when the cost
/// needs it, or when sacrificing it keeps more later costs payable.
///
/// [CR 111.10a]: https://yawgatog.com/resources/magic-rules/#R11110a
#[inline]
pub fn is_sacrificed(a: &Ability) -> bool {
    a.kind() == AbilityKind::Mana
        && a.cost().mana.is_zero()
        && matches!(
            a.cost().parts(),
            [
                CostPart::This(CostAction::Tap),
                CostPart::This(CostAction::Sacrifice)
            ] | [
                CostPart::This(CostAction::Sacrifice),
                CostPart::This(CostAction::Tap)
            ]
        )
}
