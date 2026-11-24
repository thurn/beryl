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
