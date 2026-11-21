// Generated from the card store by `just card header`: notes go below the end marker.
//! Seachrome Coast
//! Land
//!
//! This land enters tapped unless you control two or fewer other lands.
//! {T}: Add {W} or {U}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::SeachromeCoast
    .abilities(&[
        Ability::replacing(&Replacement::enters_tapped_unless_few_lands(2)),
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
    ];
}
