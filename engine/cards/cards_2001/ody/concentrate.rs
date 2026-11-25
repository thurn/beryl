// Generated from the card store by `just card header`: notes go below the end marker.
//! Concentrate {2}{U}{U}
//! Sorcery
//!
//! Draw three cards.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Concentrate
    .abilities(&[Ability::spell().effect(&draw(you(), 3))])
    .card();
