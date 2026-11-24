// Generated from the card store by `just card header`: notes go below the end marker.
//! Palace Guard {2}{W}
//! Creature — Human Soldier, 1/4
//!
//! This creature can block any number of creatures.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::PalaceGuard
    .abilities(&[Ability::rule_change(&RuleChange {
        players: Players::You,
        what: Governed::BlockAnyNumber(Objects::This),
        when: When::AsUsual,
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;
    use beryl_scenario_types::advance_to;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new(
            "Palace Guard blocks two attackers and its controller chooses which one dies",
        )
        .interaction(&[Card::SavannahLions])
        .battlefield(P0, &[Prop::Vanilla, Card::SavannahLions])
        .battlefield(P1, &[Card::PalaceGuard])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(Prop::Vanilla),
            ME.attacks_with(Card::SavannahLions),
            THEM.chooses(DecisionKind::Blocker, Card::PalaceGuard),
            THEM.chooses(DecisionKind::BlockTarget, Prop::Vanilla),
            THEM.chooses(DecisionKind::Blocker, Card::PalaceGuard),
            THEM.chooses(DecisionKind::DamageDivision, Card::SavannahLions),
            advance_to(1, TurnStep::CombatDamage),
            THEM.life(20),
            THEM.has(Zone::Graveyard, Card::PalaceGuard, 1),
            ME.has(Zone::Graveyard, Card::SavannahLions, 1),
            ME.controls(Prop::Vanilla, 1),
        ]),
        Scenario::new("Palace Guard can block a single attacker")
            .battlefield(P0, &[Prop::Vanilla])
            .battlefield(P1, &[Card::PalaceGuard])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Prop::Vanilla),
                THEM.chooses(DecisionKind::Blocker, Card::PalaceGuard),
                advance_to(1, TurnStep::CombatDamage),
                THEM.life(20),
                THEM.controls(Card::PalaceGuard, 1),
                ME.has(Zone::Graveyard, Prop::Vanilla, 1),
            ]),
    ];
}
