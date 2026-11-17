// Generated from the card store by `just card header`: notes go below the end marker.
//! Snow-Covered Island
//! Basic Snow Land — Island
//!
//! ({T}: Add {U}.)
// End of the generated card header.

use crate::prelude::*;

/// Implemented for scenarios that distinguish the snow supertype from card types.
pub static CARD: CardDef = Printed::SnowCoveredIsland
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::Blue))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SNOW_ISLAND: &str = Card::SnowCoveredIsland;

    pub const SCENARIOS: &[Scenario] = &[Scenario::new("a snow Island pays for a blue spell")
        .interaction(&[Card::MerfolkOfThePearlTrident])
        .hand(P0, &[SNOW_ISLAND, Card::MerfolkOfThePearlTrident])
        .actions(&[
            ME.plays(SNOW_ISLAND),
            ME.casts(Card::MerfolkOfThePearlTrident),
            Resolve,
            ME.tapped(SNOW_ISLAND, 1),
            ME.controls(Card::MerfolkOfThePearlTrident, 1),
        ])];
}
