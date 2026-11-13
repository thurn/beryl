// Generated from the card store by `just card header`: notes go below the end marker.
//! Stock Up {2}{U}
//! Sorcery
//!
//! Look at the top five cards of your library. Put two of them into your hand and the rest on the
//! bottom of your library in any order.
// End of the generated card header.

use crate::prelude::*;

/// The two for the hand are chosen one at a time and go there together; with two or fewer
/// cards left to choose from, they all go unasked. The rest go to the bottom one at a time,
/// each under the ones before, which is every order
/// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "If an effect puts two or
/// more cards in a specific position in a library at the same time, the owner of those cards
/// may arrange them in any order." With fewer than five cards in the library, the player
/// looks at them all.
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
        // Four cards: two to the hand, and the order of the other two on the bottom.
        Scenario::new("Stock Up with four cards in the library")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[Card::StockUp])
            .library(
                P0,
                &[
                    Card::Plains,
                    Prop::Artifact,
                    Card::Island,
                    Card::Counterspell,
                ],
            )
            .actions(&[
                ME.casts(Card::StockUp),
                Resolve,
                ME.picks(Prop::Artifact),
                ME.picks(Card::Counterspell),
                ME.picks(Card::Island),
                ME.has(Zone::Hand, Prop::Artifact, 1),
                ME.has(Zone::Hand, Card::Counterspell, 1),
                ME.library_top(&[Card::Island, Card::Plains]),
            ]),
        // Two cards: both go to the hand, with nothing to choose.
        Scenario::new("Stock Up with two cards in the library")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[Card::StockUp])
            .library(P0, &[Prop::Artifact, Card::Plains])
            .actions(&[
                ME.casts(Card::StockUp),
                Resolve,
                ME.has(Zone::Hand, Prop::Artifact, 1),
                ME.has(Zone::Hand, Card::Plains, 1),
                ME.has_cards(Zone::Library, 0),
            ]),
    ];
}
