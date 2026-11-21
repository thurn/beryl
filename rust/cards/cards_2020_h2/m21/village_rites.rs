// Generated from the card store by `just card header`: notes go below the end marker.
//! Village Rites {B}
//! Instant
//!
//! As an additional cost to cast this spell, sacrifice a creature.
//! Draw two cards.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::VillageRites
    .additional_cost(&[CostPart::sacrifice(Filter::Types(Types::CREATURE))])
    .abilities(&[Ability::spell().effect(&draw(you(), 2))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("sacrificing a creature pays for drawing two cards")
            .battlefield(P0, &[Card::Swamp, Prop::Vanilla])
            .hand(P0, &[Card::VillageRites])
            .library(P0, &[Card::Island; 3])
            .actions(&[
                ME.casts(Card::VillageRites),
                ME.has(Zone::Graveyard, Prop::Vanilla, 1),
                ME.controls(Prop::Vanilla, 0),
                Resolve,
                ME.has(Zone::Hand, Card::Island, 2),
            ]),
        Scenario::new("a land cannot pay the creature sacrifice")
            .battlefield(P0, &[Card::Swamp])
            .hand(P0, &[Card::VillageRites, Card::Island])
            .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
    ];
}
