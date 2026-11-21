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

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("discarding a card pays for drawing two cards")
                .battlefield(P0, &[Card::Mountain; 2])
                .hand(P0, &[Card::ThrillOfPossibility, Card::Swamp])
                .library(P0, &[Card::Island; 3])
                .actions(&[
                    ME.casts(Card::ThrillOfPossibility),
                    ME.has(Zone::Graveyard, Card::Swamp, 1),
                    ME.has(Zone::Hand, Card::Swamp, 0),
                    Resolve,
                    ME.has(Zone::Hand, Card::Island, 2),
                ]),
        ];
}
