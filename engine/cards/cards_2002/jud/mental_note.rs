// Generated from the card store by `just card header`: notes go below the end marker.
//! Mental Note {U}
//! Instant
//!
//! Mill two cards.
//! Draw a card.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::MentalNote
    .abilities(&[Ability::spell().effect(&sequence(&[mill(you(), 2), draw(you(), 1)]))])
    .card();
