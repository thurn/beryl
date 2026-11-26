// Generated from the card store by `just card header`: notes go below the end marker.
//! Telling Time {1}{U}
//! Instant
//!
//! Look at the top three cards of your library. Put one of those cards into your hand, one on top
//! of your library, and one on the bottom of your library.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::TellingTime
    .abilities(&[Ability::spell().effect(&dig_with_top(
        you(),
        3,
        Taken::Put(1),
        1,
        LibraryOrder::Any,
    ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Telling Time with two cards puts one in hand and one on top")
            .battlefield(P0, &[Card::Island; 2])
            .hand(P0, &[Card::TellingTime])
            .library(P0, &[Card::Plains, Card::Swamp])
            .actions(&[
                ME.casts(Card::TellingTime),
                Resolve,
                ME.picks(Card::Plains),
                ME.has(Zone::Hand, Card::Plains, 1),
                ME.library_top(&[Card::Swamp]),
            ]),
        Scenario::new("Telling Time with one card puts it into hand")
            .battlefield(P0, &[Card::Island; 2])
            .hand(P0, &[Card::TellingTime])
            .library(P0, &[Card::Plains])
            .actions(&[
                ME.casts(Card::TellingTime),
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 1),
                ME.has_cards(Zone::Library, 0),
            ]),
    ];
}
