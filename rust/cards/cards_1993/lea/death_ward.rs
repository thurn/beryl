// Generated from the card store by `just card header`: notes go below the end marker.
//! Death Ward {W}
//! Instant
//!
//! Regenerate target creature.
// End of the generated card header.

use crate::prelude::*;

/// Implemented so destroy effects that prohibit regeneration can be tested on any creature,
/// including a land creature.
pub static CARD: CardDef = Printed::DeathWard
    .abilities(&[Ability::spell()
        .target(&TargetSpec::creature())
        .effect(&regenerate(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const WARD: &str = Card::DeathWard;

    pub const SCENARIOS: &[Scenario] = &[
        // P0 shields its own creature, then destroys it with Get Lost: the shield applies, so
        // the creature stays, tapped, and its controller still gets the two Maps.
        Scenario::new("Death Ward saves a creature from Get Lost")
            .interaction(&[Card::GetLost])
            .battlefield(
                P0,
                &[Card::Plains, Card::Plains, Card::Plains, Prop::Vanilla],
            )
            .hand(P0, &[WARD, Card::GetLost])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(WARD),
                Resolve,
                ME.casts(Card::GetLost),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.tapped(Prop::Vanilla, 1),
                ME.controls(crate::tokens::MAP_TOKEN, 2),
            ]),
        // Dryad Arbor is a land and a creature, so Death Ward can shield it, and the shield
        // saves it from Strip Mine.
        Scenario::new("Death Ward saves a land creature from Strip Mine")
            .interaction(&[Card::StripMine])
            .battlefield(P0, &[Card::Plains, Card::StripMine])
            .hand(P0, &[WARD])
            .battlefield(P1, &[Card::DryadArbor])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(WARD),
                Resolve,
                ME.activates(Card::StripMine, 1),
                ME.targets(Card::DryadArbor),
                Resolve,
                THEM.controls(Card::DryadArbor, 1),
                ME.has(Zone::Graveyard, Card::StripMine, 1),
                THEM.has(Zone::Graveyard, Card::DryadArbor, 0),
            ]),
    ];
}
