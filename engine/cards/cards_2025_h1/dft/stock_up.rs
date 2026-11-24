// Generated from the card store by `just card header`: notes go below the end marker.
//! Stock Up {2}{U}
//! Sorcery
//!
//! Look at the top five cards of your library. Put two of them into your hand and the rest on the
//! bottom of your library in any order.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::StockUp
    .abilities(&[Ability::spell().effect(&dig(you(), 5, Taken::Put(2), LibraryOrder::Any))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // The artifact and Counterspell to the hand; Oust, then Plains, then Island to the
        // bottom, each under the one before. The sixth card, Tundra, is now on top.
        Scenario::new("Stock Up takes two of five and orders the rest on the bottom")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[Card::StockUp])
            .library(
                P0,
                &[
                    Card::Island,
                    Card::Plains,
                    Prop::Artifact,
                    Card::Counterspell,
                    Prop::Sorcery,
                    Card::Tundra,
                ],
            )
            .actions(&[
                ME.casts(Card::StockUp),
                Resolve,
                ME.deciding(DecisionKind::ChooseObject, 5),
                ME.picks(Prop::Artifact),
                ME.picks(Card::Counterspell),
                ME.picks(Prop::Sorcery),
                ME.picks(Card::Plains),
                ME.has(Zone::Hand, Prop::Artifact, 1),
                ME.has(Zone::Hand, Card::Counterspell, 1),
                ME.library_top(&[Card::Tundra, Prop::Sorcery, Card::Plains, Card::Island]),
            ]),
    ];
}
