// Generated from the card store by `just card header`: notes go below the end marker.
//! Double Major {G}{U}
//! Instant
//!
//! Copy target creature spell you control, except it isn't legendary if the spell is legendary. (A
//! copy of a creature spell becomes a token.)
// End of the generated card header.

use crate::prelude::*;

/// Nonlegendary is part of the copy's copiable values. [CR 707.9b] says “The final set of
/// values for that characteristic becomes part of the copiable values of the copy.”
///
/// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
pub static CARD: CardDef = Printed::DoubleMajor
    .abilities(&[Ability::spell()
        .target(&TargetSpec::one(TargetKind::Spell, creature_you_control()))
        .effect(&copy_spell_except_keeping_targets(
            target(),
            ExceptIx::new(0),
        ))])
    .excepts(&[CopyException::NOT_LEGENDARY])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const DOUBLE_MAJOR: &str = Card::DoubleMajor;
    const CAELORNA: &str = Card::CaelornaCoralTyrant;

    pub const SCENARIOS: &[Scenario] = &[
        // The copy of a legendary creature spell isn't legendary, so the legend rule keeps the
        // token and the card. Reki sees only the one cast: the copy is not cast. Reki is cast
        // first, so it is not on the battlefield when the scenario starts.
        Scenario::new("Double Major's copy of a legendary creature spell is not legendary")
            .interaction(&[Card::RekiTheHistoryOfKamigawa])
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                ],
            )
            .hand(
                P0,
                &[Card::RekiTheHistoryOfKamigawa, CAELORNA, DOUBLE_MAJOR],
            )
            .library(P0, &[Card::Plains; 3])
            .actions(&[
                ME.casts(Card::RekiTheHistoryOfKamigawa),
                Resolve,
                ME.casts(CAELORNA),
                ME.casts(DOUBLE_MAJOR),
                Resolve,
                Resolve,
                ME.token_pt(CAELORNA, 0, 8),
                ME.controls(CAELORNA, 1),
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 1),
                Resolve,
                ME.controls(CAELORNA, 2),
                ME.has(Zone::Hand, Card::Plains, 1),
            ]),
    ];
}
