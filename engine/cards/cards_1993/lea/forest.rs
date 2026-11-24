// Generated from the card store by `just card header`: notes go below the end marker.
//! Forest
//! Basic Land — Forest
//!
//! ({T}: Add {G}.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Forest
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::Green))])
    .card();
