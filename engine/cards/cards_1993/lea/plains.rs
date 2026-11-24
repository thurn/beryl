// Generated from the card store by `just card header`: notes go below the end marker.
//! Plains
//! Basic Land — Plains
//!
//! ({T}: Add {W}.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Plains
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::White))])
    .card();
