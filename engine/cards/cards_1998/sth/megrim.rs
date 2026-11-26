// Generated from the card store by `just card header`: notes go below the end marker.
//! Megrim {2}{B}
//! Enchantment
//!
//! Whenever an opponent discards a card, this enchantment deals 2 damage to that player.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Megrim
    .abilities(&[Ability::triggered(&Trigger::discards(each_opponent()))
        .effect(&deal_damage_to(that_player(), 2))])
    .card();
