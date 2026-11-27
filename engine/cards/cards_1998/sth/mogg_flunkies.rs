// Generated from the card store by `just card header`: notes go below the end marker.
//! Mogg Flunkies {1}{R}
//! Creature — Goblin, 3/3
//!
//! This creature can't attack or block alone.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::MoggFlunkies
    .abilities(&[Ability::rule_change(&RuleChange {
        players: Players::You,
        what: Governed::Alone {
            attack: true,
            block: true,
            creatures: Objects::This,
        },
        when: When::AsUsual,
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Mogg Flunkies can't attack alone")
            .battlefield(P0, &[Card::MoggFlunkies])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(20),
            ]),
        Scenario::new("Mogg Flunkies can't block alone")
            .battlefield(P0, &[Card::MoggFlunkies])
            .battlefield(P1, &[Prop::Vanilla])
            .library(P0, &[Card::Mountain, Card::Mountain])
            .library(P1, &[Card::Mountain, Card::Mountain])
            .actions(&[
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.attacks_with(Prop::Vanilla),
                advance_to(2, TurnStep::PostcombatMain),
                ME.life(18),
            ]),
    ];
}
