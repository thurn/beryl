// Generated from the card store by `just card header`: notes go below the end marker.
//! Death Ward {W}
//! Instant
//!
//! Regenerate target creature.
// End of the generated card header.

use crate::prelude::*;

/// It is in no deck: it was implemented for generated scenarios that need a regeneration shield
/// on any creature, a land creature among them, so that a destroy effect's "can't be
/// regenerated" is tested whether it destroys a creature or a land.
///
/// "Regenerate target creature." makes one regeneration shield ([CR 701.19a]): "The next time
/// [permanent] would be destroyed this turn, instead remove all damage marked on it and its
/// controller taps it. If it's an attacking or blocking creature, remove it from combat."
///
/// [CR 701.19a]: https://yawgatog.com/resources/magic-rules/#R70119a
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
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Prop::SmallCreature,
                ],
            )
            .hand(P0, &[WARD, Card::GetLost])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(WARD),
                Resolve,
                ME.casts(Card::GetLost),
                Resolve,
                ME.controls(Prop::SmallCreature, 1),
                ME.tapped(Prop::SmallCreature, 1),
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
