// Generated from the card store by `just card header`: notes go below the end marker.
//! Brainstorm {U}
//! Instant
//!
//! Draw three cards, then put two cards from your hand on top of your library in any order.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Brainstorm
    .abilities(&[Ability::spell().effect(&sequence(&[draw(you(), 3), put_back(you(), 2)]))])
    .card();
