use crate::{Ability, Effect, Players, Trigger, TurnStep};

/// "At the beginning of each end step"
pub const EACH_END_STEP: Trigger = Trigger::step(TurnStep::End, Players::EachPlayer);

/// "At the beginning of the next end step"
pub const NEXT_END_STEP: Trigger = EACH_END_STEP.next();

/// "At the beginning of your upkeep, {effect}"
pub const fn at_your_upkeep(effect: &'static Effect) -> Ability {
    Ability::triggered(&const { Trigger::step(TurnStep::Upkeep, Players::You) }).effect(effect)
}

/// "At the beginning of each opponent's upkeep, {effect}"
pub const fn at_each_opponents_upkeep(effect: &'static Effect) -> Ability {
    Ability::triggered(&const { Trigger::step(TurnStep::Upkeep, Players::EachOpponent) })
        .effect(effect)
}

/// "When this creature enters, {effect}"
pub const fn when_enters(effect: &'static Effect) -> Ability {
    Ability::triggered(&Trigger::ENTERS).effect(effect)
}

/// "Whenever this creature attacks, {effect}"
pub const fn whenever_attacks(effect: &'static Effect) -> Ability {
    Ability::triggered(&Trigger::ATTACKS).effect(effect)
}
