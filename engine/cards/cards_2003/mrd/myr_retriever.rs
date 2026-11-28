// Generated from the card store by `just card header`: notes go below the end marker.
//! Myr Retriever {2}
//! Artifact Creature — Myr, 1/1
//!
//! When this creature dies, return another target artifact card from your graveyard to your hand.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::MyrRetriever
    .abilities(&[Ability::triggered(&Trigger::DIES)
        .target(
            &TargetSpec::graveyard_card(Filter::All(&[Filter::Types(Types::ARTIFACT), yours()]))
                .other(),
        )
        .effect(&return_to_hand(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Myr Retriever does not trigger when returned to hand")
            .battlefield(P0, &[Card::MyrRetriever])
            .graveyard(P0, &[Prop::Artifact])
            .battlefield(P1, &[Card::Island, Card::Island, Card::Island])
            .hand(P1, &[Prop::Removal])
            .actions(&[
                THEM.casts(Prop::Removal),
                ME.has(Zone::Hand, Card::MyrRetriever, 1),
                ME.has(Zone::Graveyard, Prop::Artifact, 1),
                stack(0),
            ]),
        Scenario::new("Myr Retriever returns another artifact from graveyard when it dies")
            .battlefield(P0, &[Card::MyrRetriever])
            .graveyard(P0, &[Prop::Artifact])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Card::MyrRetriever),
                THEM.chooses(DecisionKind::Blocker, Prop::Vanilla),
                advance_to(1, TurnStep::CombatDamage),
                Resolve,
                ME.has(Zone::Graveyard, Card::MyrRetriever, 1),
                ME.has(Zone::Hand, Prop::Artifact, 1),
            ]),
    ];
}
