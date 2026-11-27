// Generated from the card store by `just card header`: notes go below the end marker.
//! Mana Flare {2}{R}
//! Enchantment
//!
//! Whenever a player taps a land for mana, that player adds one mana of any type that land
//! produced.
// End of the generated card header.

use crate::prelude::*;

static EXTRA_MANA: ManaChange = ManaChange::Adds {
    tapped: Tapped::A(Filter::Types(Types::LAND)),
    added: Added::TypeProduced,
};

pub static CARD: CardDef = Printed::ManaFlare
    .abilities(&[Ability::mana_change(&EXTRA_MANA)])
    .card();
