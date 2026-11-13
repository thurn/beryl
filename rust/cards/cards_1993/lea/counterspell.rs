// Generated from the card store by `just card header`: notes go below the end marker.
//! Counterspell {U}{U}
//! Instant
//!
//! Counter target spell.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Counterspell
    .abilities(&[Ability::spell()
        .target(&TargetSpec::spell())
        .effect(&counter(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // The artifact goes to its owner's graveyard, not exile, and never reaches the
        // battlefield.
        Scenario::new("Counterspell counters a spell into its owner's graveyard")
            .battlefield(P0, &[Card::Island, Card::Island])
            .hand(P0, &[Card::Counterspell])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Prop::Artifact, Card::Plains])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(Prop::Artifact),
                ME.casts(Card::Counterspell),
                THEM.has(Zone::Graveyard, Prop::Artifact, 1),
                THEM.has(Zone::Battlefield, Prop::Artifact, 0),
                ME.has(Zone::Graveyard, Card::Counterspell, 1),
            ]),
        // Two spells on the stack: P0's Counterspell targets the one beneath, P1's Counterspell
        // that targets the artifact, so the artifact resolves. A Plains pays for the artifact,
        // the Islands for Counterspell.
        Scenario::new("Counterspell chooses among spells on the stack")
            .battlefield(P0, &[Card::Island, Card::Island, Card::Plains])
            .hand(P0, &[Prop::Artifact, Card::Counterspell, Card::Plains])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Card::Counterspell])
            .actions(&[
                ME.casts(Prop::Artifact),
                ME.passes(),
                THEM.casts(Card::Counterspell),
                ME.casts(Card::Counterspell),
                ME.targets(Card::Counterspell),
                Resolve,
                THEM.has(Zone::Graveyard, Card::Counterspell, 1),
                Resolve,
                ME.has(Zone::Battlefield, Prop::Artifact, 1),
            ]),
    ];
}
