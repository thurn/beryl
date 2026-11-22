//! Matches triggers that wait for a step or phase to begin.
//!
//! Implements: none. See CR 500.6 for when these abilities trigger.

use beryl_card::TriggerEvent;

use crate::state::TurnStep;

/// Whether a trigger waiting for the beginning of a step or phase waits for `step` (see [CR 500.6]):
/// "When a phase or step begins, any abilities that trigger 'at the beginning of' that phase or
/// step trigger."
///
/// [CR 500.6]: https://yawgatog.com/resources/magic-rules/#R5006
pub(in crate::stack) fn begins(
    event: TriggerEvent,
    step: TurnStep,
    first: impl FnOnce() -> bool,
) -> bool {
    match event {
        TriggerEvent::Step(at) => at == step,
        TriggerEvent::FirstStep(at) => at == step && first(),
        // see CR 505.1: "There are two main phases in a turn."
        TriggerEvent::MainPhase => {
            matches!(step, TurnStep::PrecombatMain | TurnStep::PostcombatMain)
        }
        TriggerEvent::Enters
        | TriggerEvent::Dies
        | TriggerEvent::Leaves
        | TriggerEvent::Attacks
        | TriggerEvent::PlayerAttacks
        | TriggerEvent::DealsCombatDamageToPlayer
        | TriggerEvent::CreatesTokens
        | TriggerEvent::Casts
        | TriggerEvent::CastsFrom(_)
        | TriggerEvent::Discards
        | TriggerEvent::Draws
        | TriggerEvent::BecomesTarget
        | TriggerEvent::BecomesTargetOfSpell
        | TriggerEvent::Mutates
        | TriggerEvent::DealsDamage
        | TriggerEvent::TurnedFaceUp
        | TriggerEvent::ClassLevel(_)
        | TriggerEvent::Chapter { .. }
        | TriggerEvent::State(_) => false,
    }
}
