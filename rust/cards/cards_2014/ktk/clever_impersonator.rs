// Generated from the card store by `just card header`: notes go below the end marker.
//! Clever Impersonator {2}{U}{U}
//! Creature — Shapeshifter, 0/0
//!
//! You may have this creature enter as a copy of any nonland permanent on the battlefield.
// End of the generated card header.

use crate::prelude::*;

/// The Impersonator may copy any nonland permanent as it enters, including one that isn't a
/// creature. It is implemented for scenarios about the types of an entering permanent.
pub static CARD: CardDef = Printed::CleverImpersonator
    .abilities(&[Ability::replacing(&Replacement::enter_as_copy(
        &Filter::Not(&Filter::Types(Types::LAND)),
        None,
    ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const IMPERSONATOR: &str = Card::CleverImpersonator;

    pub const SCENARIOS: &[Scenario] = &[
        // The copy can be a noncreature artifact even when a creature is also available.
        Scenario::new("Clever Impersonator can enter as a noncreature artifact")
            .hand(P0, &[IMPERSONATOR])
            .battlefield(P0, &[Card::Island; 4])
            .battlefield(P1, &[Prop::Equipment, Prop::Vanilla])
            .actions(&[
                ME.casts(IMPERSONATOR),
                Resolve,
                ME.picks_up_to(Prop::Equipment),
                ME.controls(Prop::Equipment, 1),
            ]),
        // The copy is optional, and the unmodified 0/0 goes to the graveyard.
        Scenario::new("Clever Impersonator can enter without copying")
            .hand(P0, &[IMPERSONATOR])
            .battlefield(P0, &[Card::Island; 4])
            .battlefield(P1, &[Prop::Equipment])
            .actions(&[
                ME.casts(IMPERSONATOR),
                Resolve,
                ME.done_picking(),
                ME.has(Zone::Graveyard, IMPERSONATOR, 1),
            ]),
    ];
}
