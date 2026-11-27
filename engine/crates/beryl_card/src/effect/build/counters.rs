//! Builders for counters on objects and mana in a player's pool.

use crate::counter::CounterKind;
use crate::effect::{Amount, Effect, Objects, Players};
use crate::mana::{ManaColor, ManaSet};

/// [`Effect::AddCounters`]: "put [count] [kind] counters on [objects]".
pub const fn add_counters(objects: Objects, kind: CounterKind, count: u8) -> Effect {
    Effect::AddCounters {
        objects,
        kind,
        count: Amount::Fixed(count),
    }
}

/// [`Effect::RemoveAllCounters`]: "remove all counters from [objects]".
pub const fn remove_all_counters(objects: Objects) -> Effect {
    Effect::RemoveAllCounters(objects)
}

/// [`Effect::DoubleCounters`]: "double the number of [kind] counters on [objects]".
pub const fn double_counters(objects: Objects, kind: CounterKind) -> Effect {
    Effect::DoubleCounters { objects, kind }
}

/// [`Effect::RemoveCounters`]: "remove [count] [kind] counters from [objects]".
pub const fn remove_counters(objects: Objects, kind: CounterKind, count: u8) -> Effect {
    Effect::RemoveCounters {
        objects,
        kind,
        count: Amount::Fixed(count),
    }
}

/// [`Effect::AddMana`]: "[player] adds [mana]".
pub const fn add_mana(player: Players, mana: ManaSet) -> Effect {
    Effect::AddMana { player, mana }
}

/// [`Effect::AddManaOfKind`]: "[player] adds [amount] mana of [kind]".
pub const fn add_mana_of_kind(player: Players, kind: ManaColor, amount: Amount) -> Effect {
    Effect::AddManaOfKind {
        player,
        kind,
        amount,
    }
}
