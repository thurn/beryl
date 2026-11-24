// Generated from the card store by `just card header`: notes go below the end marker.
//! Island
//! Basic Land — Island
//!
//! ({T}: Add {U}.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Island
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::Blue))])
    .card();
