// Generated from the card store by `just card header`: notes go below the end marker.
//! Thrill of Possibility {1}{R}
//! Instant
//!
//! As an additional cost to cast this spell, discard a card.
//! Draw two cards.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ThrillOfPossibility
    .additional_cost(&[CostPart::discard(Filter::Any)])
    .abilities(&[Ability::spell().effect(&draw(you(), 2))])
    .card();
