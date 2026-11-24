// Generated from the card store by `just card header`: notes go below the end marker.
//! Asphodel Wanderer {B}
//! Creature — Skeleton Soldier, 1/1
//!
//! {2}{B}: Regenerate this creature.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::AsphodelWanderer
    .abilities(&[Ability::activated(Cost::mana(mana("{2}{B}"))).effect(&regenerate(this()))])
    .card();
