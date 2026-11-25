//! Builders for the order of instructions and the choices between them: sequences,
//! "if", "may", "unless", choosing objects, and delayed abilities.

use crate::cost::Cost;
use crate::effect::{Amount, Condition, Effect, Objects, Players};
use crate::index::AbilityIx;
use crate::mana::ManaCost;
use crate::prompt::Prompt;

/// [`Effect::Sequence`]: each instruction in turn.
pub const fn sequence(effects: &'static [Effect]) -> Effect {
    Effect::Sequence(effects)
}

/// [`Effect::If`] with nothing otherwise: "if [condition], [then]".
pub const fn if_then(condition: Condition, then: &'static Effect) -> Effect {
    Effect::If {
        condition,
        then,
        otherwise: None,
    }
}

/// [`Effect::If`]: "if [condition], [then]. Otherwise, [otherwise]."
pub const fn if_else(
    condition: Condition,
    then: &'static Effect,
    otherwise: &'static Effect,
) -> Effect {
    Effect::If {
        condition,
        then,
        otherwise: Some(otherwise),
    }
}

/// [`Effect::May`]: "[player] may [effect]".
pub const fn may(player: Players, effect: &'static Effect) -> Effect {
    Effect::May { player, effect }
}

/// [`Effect::Unless`] of mana: "[effect] unless [player] pays [cost]".
pub const fn unless_pays(effect: &'static Effect, player: Players, cost: ManaCost) -> Effect {
    unless(effect, player, Cost::mana(cost))
}

/// [`Effect::Unless`]: "[effect] unless [player] [pays a cost]".
pub const fn unless(effect: &'static Effect, player: Players, cost: Cost) -> Effect {
    Effect::Unless {
        effect,
        player,
        cost,
    }
}

/// [`Effect::ForEachPlayer`]: `effect` for each of `players`, who is [`that_player`](super::that_player) in it.
pub const fn for_each_player(players: Players, effect: &'static Effect) -> Effect {
    Effect::ForEachPlayer { players, effect }
}

/// [`Effect::Choose`]: "[player] chooses one of [among]", for what `prompt` says.
pub const fn choose(player: Players, among: Objects, prompt: Prompt) -> Effect {
    Effect::Choose {
        player,
        among,
        prompt,
        optional: false,
        hidden: false,
    }
}

/// [`Effect::Choose`], optional: "[player] may choose one of [among]", for what `prompt`
/// says.
pub const fn may_choose(player: Players, among: Objects, prompt: Prompt) -> Effect {
    Effect::Choose {
        player,
        among,
        prompt,
        optional: true,
        hidden: false,
    }
}

/// [`Effect::Choose`], optional and hidden: "[player] may choose one of [among]", without
/// revealing which until the effect goes on (CR 101.4a), for what `prompt` says.
pub const fn may_choose_hidden(player: Players, among: Objects, prompt: Prompt) -> Effect {
    Effect::Choose {
        player,
        among,
        prompt,
        optional: true,
        hidden: true,
    }
}

/// [`Effect::LookAt`]: "[player] looks at [cards]".
pub const fn look_at(player: Players, cards: Objects) -> Effect {
    Effect::LookAt { player, cards }
}

/// [`Effect::FlipCoin`]: "[player] flips a coin".
pub const fn flip_coin(player: Players) -> Effect {
    Effect::FlipCoin(player)
}

/// [`Effect::Delay`] carrying nothing: "[when], [effect]", the delayed triggered ability
/// that is the `ability`th of its face, counting from 0.
pub const fn delay(ability: u8) -> Effect {
    Effect::Delay {
        ability: AbilityIx::new(ability),
        carrying: None,
        object: None,
    }
}

/// [`Effect::Delay`] carrying a number: "[when], [effect]", the delayed triggered ability
/// that is the `ability`th of its face, counting from 0, which reads `carrying` as
/// [`carried`](super::carried).
pub const fn delay_carrying(ability: u8, carrying: Amount) -> Effect {
    Effect::Delay {
        ability: AbilityIx::new(ability),
        carrying: Some(carrying),
        object: None,
    }
}

/// [`Effect::Delay`] carrying an object: "[when], [effect]", the delayed triggered ability
/// that is the `ability`th of its face, which reads `object` as [`carried_object`](super::carried_object).
pub const fn delay_with(ability: u8, object: Objects) -> Effect {
    Effect::Delay {
        ability: AbilityIx::new(ability),
        carrying: None,
        object: Some(object),
    }
}
