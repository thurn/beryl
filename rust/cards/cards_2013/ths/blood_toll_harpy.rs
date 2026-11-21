// Generated from the card store by `just card header`: notes go below the end marker.
//! Blood-Toll Harpy {2}{B}
//! Creature — Harpy, 2/1
//!
//! Flying
//! When this creature enters, each player loses 1 life.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::BloodTollHarpy
    .keywords(Keywords::FLYING)
    .abilities(&[when_enters(&lose_life(each_player(), 1))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("another permanent entering leaves both life totals unchanged")
                .battlefield(P0, &[Card::BloodTollHarpy])
                .hand(P0, &[Card::Island])
                .actions(&[ME.plays(Card::Island), stack(0), ME.life(20), THEM.life(20)]),
        ];
}
