// Generated from the card store by `just card header`: notes go below the end marker.
//! Silent Arbiter {4}
//! Artifact Creature — Construct, 1/5
//!
//! No more than one creature can attack each combat.
//! No more than one creature can block each combat.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::SilentArbiter
    .abilities(&[
        Ability::rule_change(&RuleChange {
            players: Players::EachPlayer,
            what: Governed::CombatLimit {
                attackers: Some(1),
                blockers: None,
            },
            when: When::AsUsual,
        }),
        Ability::rule_change(&RuleChange {
            players: Players::EachPlayer,
            what: Governed::CombatLimit {
                attackers: None,
                blockers: Some(1),
            },
            when: When::AsUsual,
        }),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("No more than one creature can attack each combat")
            .battlefield(P0, &[Card::SilentArbiter, Prop::Vanilla, Prop::Vanilla])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Vanilla),
                advance_to(1, TurnStep::DeclareAttackers),
                ME.attacking(Prop::Vanilla, 1),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(18),
            ]),
        Scenario::new("No more than one creature can block each combat")
            .at(2, P1, TurnStep::BeginningOfCombat)
            .battlefield(P0, &[Card::SilentArbiter, Prop::Vanilla, Prop::Vanilla])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                THEM.attacks_with(Prop::Vanilla),
                ME.chooses(DecisionKind::Blocker, Prop::Vanilla),
                advance_to(2, TurnStep::CombatDamage),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                ME.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
    ];
}
