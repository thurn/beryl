//! Builders for instructions that change the turn.

use crate::{AbilityIx, Effect, Players};

/// [`Effect::ControlPlayer`]: "you control [player] during that player's next turn".
pub const fn control_player(player: Players) -> Effect {
    Effect::ControlPlayer(player)
}

/// [`Effect::ExtraTurn`]: "[player] takes an extra turn after this one".
pub const fn extra_turn(player: Players) -> Effect {
    Effect::ExtraTurn(player, None)
}

/// An extra turn with a delayed ability tied to that turn. Use a trigger made with
/// [`crate::Trigger::during_extra_turn`] at index `ability` on this face, counting from zero.
pub const fn extra_turn_with_trigger(player: Players, ability: u8) -> Effect {
    Effect::ExtraTurn(player, Some(AbilityIx::new(ability)))
}

/// [`Effect::EndTurn`]: "End the turn".
pub const fn end_turn() -> Effect {
    Effect::EndTurn
}
