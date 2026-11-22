//! Builders for dealing and preventing damage.

use crate::effect::{Amount, Effect, Players, Prevention, Recipients};
use crate::filter::Filter;
use crate::index::SpecIx;

/// [`Effect::DealDamage`] to players: "this deals [amount] damage to [player]".
pub const fn deal_damage_to(player: Players, amount: u8) -> Effect {
    Effect::DealDamage {
        to: Recipients::Players(player),
        amount: Amount::Fixed(amount),
    }
}

/// Deals two damage divided among one or two targets chosen for the first target spec.
pub const fn deal_two_divided() -> Effect {
    Effect::DealDamage {
        to: Recipients::DividedTwo(SpecIx::new(0)),
        amount: Amount::Fixed(2),
    }
}

/// [`Effect::DealDamage`] to the first target, a player or a permanent: "this deals [amount]
/// damage to any target", with [`crate::TargetSpec::any_target`].
pub const fn deal_damage_to_any_target(amount: u8) -> Effect {
    Effect::DealDamage {
        to: Recipients::target(SpecIx::new(0)),
        amount: Amount::Fixed(amount),
    }
}

/// [`Effect::PreventDamage`] of the next `n` damage to the first target, a player or a
/// permanent: "prevent the next [n] damage that would be dealt to any target this turn", with
/// [`crate::TargetSpec::any_target`].
pub const fn prevent_next_damage_to_any_target(n: u8) -> Effect {
    Effect::PreventDamage {
        to: Recipients::target(SpecIx::new(0)),
        prevention: Prevention::Next(n),
        from: None,
    }
}

/// [`Effect::PreventDamage`] of the next damage a source of the controller's choice that
/// `filter` accepts would deal to `players`: "the next time a red source of your choice would
/// deal damage to you this turn, prevent that damage".
pub const fn prevent_next_damage_from_chosen_source(
    players: Players,
    filter: &'static Filter,
) -> Effect {
    Effect::PreventDamage {
        to: Recipients::Players(players),
        prevention: Prevention::NextTime,
        from: Some(filter),
    }
}
