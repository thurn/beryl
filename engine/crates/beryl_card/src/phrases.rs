//! Prelude functions that compose the card vocabulary.

use crate::effect::build::{its_controller, unless_pays};
use crate::{Ability, Cost, CostPart, Effect, Filter, ManaCost, Trigger, Types};

/// Matches an instant or sorcery spell or card.
pub const fn instant_or_sorcery() -> Filter {
    Filter::Types(Types::INSTANT.union(Types::SORCERY))
}

/// Tap and sacrifice this permanent to pay the activation cost.
pub const fn tap_and_sacrifice() -> Cost {
    Cost::of_parts(&[CostPart::TAP, CostPart::SACRIFICE])
}

/// When this permanent enters, do `effect`.
pub const fn when_enters(effect: &'static Effect) -> Ability {
    Ability::triggered(&Trigger::ENTERS).effect(effect)
}

/// Whenever this creature attacks, do `effect`.
pub const fn whenever_attacks(effect: &'static Effect) -> Ability {
    Ability::triggered(&Trigger::ATTACKS).effect(effect)
}

/// Do `effect` unless the first target's controller pays `mana`.
pub const fn unless_its_controller_pays(effect: &'static Effect, mana: ManaCost) -> Effect {
    unless_pays(effect, its_controller(), mana)
}
