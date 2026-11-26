// Generated from the card store by `just card header`: notes go below the end marker.
//! Island
//! Basic Land — Island
//!
//! ({T}: Add {U}.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Island
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::Blue))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new(
                "Island is played with three others and tapped for mana in a shared game",
            )
            .shared_library(&[Card::Island, Card::Island])
            .interaction(&[Card::Dandan])
            .battlefield(P0, &[Card::Island, Card::Island, Card::Island])
            .hand(P0, &[Card::Island, Card::Dandan])
            .actions(&[
                ME.plays(Card::Island),
                ME.controls(Card::Island, 4),
                ME.casts(Card::Dandan),
                Resolve,
                ME.tapped(Card::Island, 2),
            ]),
        ];
}
