// Generated from the card store by `just card header`: notes go below the end marker.
//! Spara's Headquarters
//! Land — Forest Plains Island
//!
//! ({T}: Add {G}, {W}, or {U}.)
//! This land enters tapped.
//! Cycling {3} ({3}, Discard this card: Draw a card.)
// End of the generated card header.

use crate::prelude::*;

/// Its three basic land types give it their intrinsic mana abilities ([CR 305.6]): "An object with
/// the land card type and a basic land type has the intrinsic ability '{T}: Add [mana symbol],'".
/// Cycling ([CR 702.29a]) "functions only while the card with cycling is in a player's hand."
///
/// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
/// [CR 702.29a]: https://yawgatog.com/resources/magic-rules/#R70229a
pub static CARD: CardDef = Printed::SparasHeadquarters
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::Green)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        Ability::replacing(Replacement::EntersTapped),
        Ability::cycling(mana("{3}")),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// The cycling ability is the face's fifth.
    const CYCLING: u8 = 4;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Spara's Headquarters enters tapped")
            .hand(P0, &[Card::SparasHeadquarters])
            .actions(&[
                ME.plays(Card::SparasHeadquarters),
                ME.tapped(Card::SparasHeadquarters, 1),
            ]),
        Scenario::new("Cycling Spara's Headquarters")
            .battlefield(P0, &[Card::Plains; 3])
            .hand(P0, &[Card::SparasHeadquarters])
            .actions(&[
                ME.activates(Card::SparasHeadquarters, CYCLING),
                ME.has(Zone::Hand, Card::Island, 1),
                ME.has(Zone::Graveyard, Card::SparasHeadquarters, 1),
                ME.tapped(Card::Plains, 3),
            ]),
        // Its {U} is the only blue mana for Timetwister.
        Scenario::new("Spara's Headquarters' blue mana")
            .battlefield(P0, &[Card::SparasHeadquarters, Card::Plains, Card::Plains])
            .hand(P0, &[Card::Timetwister])
            .library(P0, &[Card::Island; 7])
            .library(P1, &[Card::Island; 8])
            .actions(&[
                ME.casts(Card::Timetwister),
                ME.has(Zone::Graveyard, Card::Timetwister, 1),
                ME.tapped(Card::SparasHeadquarters, 1),
            ]),
        // Its {W} is the only white mana for the artifact. The land still to play keeps the game
        // in P0's main phase.
        Scenario::new("Spara's Headquarters' white mana")
            .battlefield(P0, &[Card::SparasHeadquarters, Card::Island])
            .hand(P0, &[Prop::Artifact, Card::Plains])
            .actions(&[
                ME.casts(Prop::Artifact),
                ME.tapped(Card::SparasHeadquarters, 1),
                Resolve,
                ME.has(Zone::Battlefield, Prop::Artifact, 1),
            ]),
    ];
}
