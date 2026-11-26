// Generated from the card store by `just card header`: notes go below the end marker.
//! Chart a Course {1}{U}
//! Sorcery
//!
//! Draw two cards. Then discard a card unless you attacked this turn.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ChartACourse
    .abilities(&[Ability::spell().effect(&sequence(&[
        draw(you(), 2),
        if_then(
            Condition::Not(&Condition::AttackedThisTurn(you())),
            &discard(you(), 1),
        ),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const CHART: &str = Card::ChartACourse;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Chart a Course does not discard if you attacked this turn")
            .hand(P0, &[CHART, Card::Plains])
            .library(P0, &[Card::Island, Card::Plains])
            .battlefield(P0, &[Prop::Vanilla, Card::Island, Card::Island])
            .actions(&[
                ME.passes(),
                ME.attacks_with(Prop::Vanilla),
                advance_to(1, TurnStep::PostcombatMain),
                ME.casts(CHART),
                Resolve,
                ME.has_cards(Zone::Hand, 3),
                ME.has_cards(Zone::Graveyard, 1),
            ]),
        Scenario::new("Chart a Course does not discard even if the attacker died in combat")
            .hand(P0, &[CHART, Card::Plains])
            .library(P0, &[Card::Island, Card::Plains])
            .battlefield(P0, &[Prop::Vanilla, Card::Island, Card::Island])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                ME.passes(),
                ME.attacks_with(Prop::Vanilla),
                THEM.chooses(DecisionKind::Blocker, Prop::Vanilla),
                advance_to(1, TurnStep::PostcombatMain),
                ME.has(Zone::Graveyard, Prop::Vanilla, 1),
                ME.casts(CHART),
                Resolve,
                ME.has_cards(Zone::Hand, 3),
                ME.has_cards(Zone::Graveyard, 2),
            ]),
    ];
}
