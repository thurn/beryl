// Generated from the card store by `just card header`: notes go below the end marker.
//! Mana Reflection {4}{G}{G}
//! Enchantment
//!
//! If you tap a permanent for mana, it produces twice as much of that mana instead.
// End of the generated card header.

use crate::prelude::*;

static DOUBLE_MANA: ManaChange = ManaChange::Multiplies {
    tapped: Filter::Any,
    by: 2,
};

pub static CARD: CardDef = Printed::ManaReflection
    .abilities(&[Ability::mana_change(&DOUBLE_MANA)])
    .card();
