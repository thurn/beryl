// Generated from the card store by `just card header`: notes go below the end marker.
//! Demonic Tutor {1}{B}
//! Sorcery
//!
//! Search your library for a card, put that card into your hand, then shuffle.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::DemonicTutor
    .abilities(&[Ability::spell().effect(&search(you(), Filter::Any, Found::ToHand))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const TUTOR: &str = Card::DemonicTutor;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Demonic Tutor must find a card from a non-empty library")
            .hand(P0, &[TUTOR])
            .battlefield(P0, &[Card::Swamp; 2])
            .library(P0, &[Card::Island, Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(TUTOR),
                Resolve,
                // Only the two library cards are offered; finding nothing is not an option.
                ME.deciding(DecisionKind::ChooseObject, 2),
                ME.picks(Card::Island),
                ME.has(Zone::Hand, Card::Island, 1),
                ME.has_cards(Zone::Library, 1),
                ME.has(Zone::Graveyard, TUTOR, 1),
            ]),
        Scenario::new("Demonic Tutor finds nothing from an empty library")
            .hand(P0, &[TUTOR])
            .battlefield(P0, &[Card::Swamp; 2])
            .library(P0, &[])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(TUTOR),
                Resolve,
                ME.has_cards(Zone::Hand, 0),
                ME.has_cards(Zone::Library, 0),
                ME.has(Zone::Graveyard, TUTOR, 1),
            ]),
    ];
}
