// Generated from the card store by `just card header`: notes go below the end marker.
//! Doubling Season {4}{G}
//! Enchantment
//!
//! If an effect would create one or more tokens under your control, it creates twice that many of
//! those tokens instead.
//! If an effect would put one or more counters on a permanent you control, it puts twice that many
//! of those counters on that permanent instead.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::DoublingSeason
    .abilities(&[
        Ability::replacing(&Replacement::tokens_created(
            Players::You,
            Rewrite::Multiply(2),
        )),
        Ability::replacing(&Replacement::counters_put(
            &Filter::Whose(&Players::You),
            None,
            true,
            Rewrite::Multiply(2),
        )),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SEASON: &str = Card::DoublingSeason;
    const SCALES: &str = Card::HardenedScales;
    const EMPEROR: &str = Card::TheWanderingEmperor;
    const BEAR: &str = Prop::Vanilla;

    pub const SCENARIOS: &[Scenario] = &[
        // Both replace the one counter differently, so its controller chooses the order
        // (CR 616.1): Hardened Scales first makes (1 + 1) × 2.
        Scenario::new("Hardened Scales applied before Doubling Season makes one counter four")
            .interaction(&[SCALES, EMPEROR])
            .battlefield(P0, &[SEASON, SCALES, EMPEROR, BEAR])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.activates(EMPEROR, 1),
                ME.target_count(1),
                Resolve,
                ME.picks(SCALES),
                ME.counters(BEAR, CounterKind::PlusOne, 4),
            ]),
        Scenario::new("Doubling Season applied before Hardened Scales makes one counter three")
            .interaction(&[SCALES, EMPEROR])
            .battlefield(P0, &[SEASON, SCALES, EMPEROR, BEAR])
            .library(P0, &[Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.activates(EMPEROR, 1),
                ME.target_count(1),
                Resolve,
                ME.picks(SEASON),
                ME.counters(BEAR, CounterKind::PlusOne, 3),
            ]),
    ];
}
