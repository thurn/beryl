// Generated from the card store by `just card header`: notes go below the end marker.
//! Monastery Swiftspear {R}
//! Creature — Human Monk, 1/2
//!
//! Haste
//! Prowess (Whenever you cast a noncreature spell, this creature gets +1/+1 until end of turn.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::MonasterySwiftspear
    .keywords(Keywords::HASTE)
    .abilities(&[Ability::prowess()])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SWIFTSPEAR: &str = Card::MonasterySwiftspear;

    pub const SCENARIOS: &[Scenario] = &[
        // Prowess lasts only until end of turn, so by the next turn's upkeep Swiftspear is 1/2
        // again.
        Scenario::new("Prowess bonus on Monastery Swiftspear ends at end of turn")
            .battlefield(P0, &[SWIFTSPEAR, Card::Island, Card::Island, Card::Island])
            .library(P0, &[Card::Island; 10])
            .library(P1, &[Card::Island; 10])
            .hand(P0, &[Prop::UntargetedSpell])
            .actions(&[
                ME.casts(Prop::UntargetedSpell),
                Resolve,
                Resolve,
                ME.pt(SWIFTSPEAR, 2, 3),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::Upkeep),
                ME.pt(SWIFTSPEAR, 1, 2),
            ]),
    ];
}
