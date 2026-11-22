//! Neighbouring values of instructions that change the turn.

use beryl_card::{AbilityIx, Effect, Players};

use super::{Kind, players};

pub(super) fn extra_turn(
    player: Players,
    ability: Option<AbilityIx>,
    add: &mut impl FnMut(Kind, String, Effect),
) {
    for (name, player) in players(player) {
        add(
            Kind::EffectPlayers,
            name,
            Effect::ExtraTurn(player, ability),
        );
    }
    if ability.is_some() {
        add(
            Kind::SequenceStep,
            "omit the turn's delayed trigger".to_owned(),
            Effect::ExtraTurn(player, None),
        );
    }
}

/// The name of the neighbour that omits an instruction which only changes the course of the
/// game or turn: adding a step, restarting the game or ending the turn.
pub(super) fn omission(effect: &Effect) -> String {
    match effect {
        Effect::AdditionalStep(_) => "omit adding the step",
        Effect::RestartGame { .. } => "omit restarting the game",
        _ => "omit ending the turn",
    }
    .to_owned()
}
