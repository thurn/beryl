// Generated from the card store by `just card header`: notes go below the end marker.
//! Tundra
//! Land — Plains Island
//!
//! ({T}: Add {W} or {U}.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Tundra
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[Scenario::new("Tundra pays for white and blue spells")
        .battlefield(P0, &[Card::Tundra, Card::Tundra, Card::Mountain])
        .hand(P0, &[Prop::Artifact, Prop::Vanilla])
        .actions(&[
            ME.casts(Prop::Artifact),
            Resolve,
            ME.has(Zone::Battlefield, Prop::Artifact, 1),
            ME.casts(Prop::Vanilla),
            Resolve,
            ME.has(Zone::Battlefield, Prop::Vanilla, 1),
        ])];
}
