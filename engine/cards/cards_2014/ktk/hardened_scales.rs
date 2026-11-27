// Generated from the card store by `just card header`: notes go below the end marker.
//! Hardened Scales {G}
//! Enchantment
//!
//! If one or more +1/+1 counters would be put on a creature you control, that many plus one +1/+1
//! counters are put on it instead.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::HardenedScales
    .abilities(&[Ability::replacing(&Replacement::counters_put(
        &creature_you_control(),
        Some(CounterKind::PlusOne),
        false,
        Rewrite::Plus(1),
    ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SCALES: &str = Card::HardenedScales;
    const EMPEROR: &str = Card::TheWanderingEmperor;
    const BEAR: &str = Prop::Vanilla;

    pub const SCENARIOS: &[Scenario] = &[
        // Each Hardened Scales adds one; additions give the same count in any order, so
        // nobody is asked.
        Scenario::new("Two Hardened Scales make one +1/+1 counter three")
            .interaction(&[EMPEROR])
            .battlefield(P0, &[SCALES, SCALES, EMPEROR, BEAR])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.activates(EMPEROR, 1),
                ME.target_count(1),
                Resolve,
                ME.counters(BEAR, CounterKind::PlusOne, 3),
            ]),
    ];
}
