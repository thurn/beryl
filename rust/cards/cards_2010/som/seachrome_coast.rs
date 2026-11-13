// Generated from the card store by `just card header`: notes go below the end marker.
//! Seachrome Coast
//! Land
//!
//! This land enters tapped unless you control two or fewer other lands.
//! {T}: Add {W} or {U}.
// End of the generated card header.

use crate::prelude::*;

/// The entering replacement ([CR 614.1d](https://yawgatog.com/resources/magic-rules/#R6141d)):
/// "Continuous effects that read "[This permanent] enters . . ." ... are replacement
/// effects." It counts the lands its controller controls as it would enter, which are all
/// other lands.
pub static CARD: CardDef = Printed::SeachromeCoast
    .abilities(&[
        Ability::replacing(Replacement::EntersTappedUnlessFewLands(2)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Then its {U} is the only blue mana for Timetwister.
        Scenario::new("Seachrome Coast with two other lands enters untapped")
            .battlefield(P0, &[Card::Plains, Card::Plains])
            .hand(P0, &[Card::SeachromeCoast, Card::Timetwister])
            .library(P0, &[Card::Island; 7])
            .library(P1, &[Card::Island; 8])
            .actions(&[
                ME.plays(Card::SeachromeCoast),
                ME.tapped(Card::SeachromeCoast, 0),
                ME.casts(Card::Timetwister),
                ME.tapped(Card::SeachromeCoast, 1),
                ME.has(Zone::Graveyard, Card::Timetwister, 1),
            ]),
        Scenario::new("Seachrome Coast with three other lands enters tapped")
            .battlefield(P0, &[Card::Plains, Card::Island, Card::Tundra])
            .hand(P0, &[Card::SeachromeCoast])
            .actions(&[
                ME.plays(Card::SeachromeCoast),
                ME.tapped(Card::SeachromeCoast, 1),
            ]),
        // Its {W} is P0's only white mana for the artifact. The land still to play keeps the
        // game in P0's main phase.
        Scenario::new("Seachrome Coast's white mana")
            .battlefield(P0, &[Card::SeachromeCoast, Card::Island])
            .hand(P0, &[Prop::Artifact, Card::Plains])
            .actions(&[
                ME.casts(Prop::Artifact),
                ME.tapped(Card::SeachromeCoast, 1),
                Resolve,
                ME.controls(Prop::Artifact, 1),
            ]),
    ];
}
