// Generated from the card store by `just card header`: notes go below the end marker.
//! Shrapnel Blast {1}{R}
//! Instant
//!
//! As an additional cost to cast this spell, sacrifice an artifact.
//! Shrapnel Blast deals 5 damage to any target.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ShrapnelBlast
    .additional_cost(&[CostPart::sacrifice(Filter::Types(Types::ARTIFACT))])
    .abilities(&[Ability::spell()
        .target(&TargetSpec::any_target())
        .effect(&deal_damage_to_any_target(5))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("Shrapnel Blast sacrifices an artifact before the opponent can respond")
                .battlefield(
                    P0,
                    &[
                        Prop::Artifact,
                        Prop::Artifact,
                        Card::Mountain,
                        Card::Mountain,
                    ],
                )
                .hand(P0, &[Card::ShrapnelBlast, Card::Plains])
                .battlefield(P1, &[Card::Island; 2])
                .hand(P1, &[Prop::Instant])
                .actions(&[
                    ME.casts(Card::ShrapnelBlast),
                    ME.targets_player(THEM),
                    ME.has(Zone::Graveyard, Prop::Artifact, 1),
                    ME.controls(Prop::Artifact, 1),
                    THEM.casts(Prop::Instant),
                    ME.has(Zone::Graveyard, Card::ShrapnelBlast, 1),
                ]),
        ];
}
