// Generated from the card store by `just card header`: notes go below the end marker.
//! Mountain
//! Basic Land — Mountain
//!
//! ({T}: Add {R}.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Mountain
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::Red))])
    .card();
