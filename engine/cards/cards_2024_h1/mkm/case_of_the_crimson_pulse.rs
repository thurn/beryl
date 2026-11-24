// Generated from the card store by `just card header`: notes go below the end marker.
//! Case of the Crimson Pulse {2}{R}
//! Enchantment — Case
//!
//! When this Case enters, discard a card, then draw two cards.
//! To solve — You have no cards in hand. (If unsolved, solve at the beginning of your end step.)
//! Solved — At the beginning of your upkeep, discard your hand, then draw two cards.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::CaseOfTheCrimsonPulse
    .abilities(&[
        Ability::triggered(&Trigger::ENTERS)
            .effect(&sequence(&[discard(you(), 1), draw(you(), 2)])),
        Ability::to_solve(&Condition::Compare(
            Amount::Count(&hand_of(&Players::You)),
            Comparison::Equal,
            Amount::Fixed(0),
        )),
        Ability::triggered(&Trigger::step(TurnStep::Upkeep, Players::You))
            .solved()
            .effect(&sequence(&[
                Effect::Discard {
                    player: Players::You,
                    count: Amount::Count(&hand_of(&Players::You)),
                },
                draw(you(), 2),
            ])),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const CASE: &str = Card::CaseOfTheCrimsonPulse;
    const WISP: &str = Card::Flickerwisp;
    const COPY: &str = Card::CleverImpersonator;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("The unsolved Case still draws when there is nothing to discard")
            .battlefield(P0, &[Card::Mountain; 3])
            .hand(P0, &[CASE])
            .library(P0, &[Card::Forest; 5])
            .actions(&[
                ME.casts(CASE),
                Resolve,
                stack(1),
                Resolve,
                ME.has_cards(Zone::Hand, 2),
            ]),
        Scenario::new("The Case solves with an empty hand and draws in upkeep")
            .battlefield(P0, &[CASE])
            .library(
                P0,
                &[
                    Card::Forest,
                    Card::Forest,
                    Card::Island,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Forest,
                ],
            )
            .library(P1, &[Card::Island; 4])
            .actions(&[
                advance_to(1, TurnStep::End),
                stack(1),
                Resolve,
                advance_to(3, TurnStep::Upkeep),
                stack(1),
                Resolve,
                ME.has(Zone::Hand, Card::Forest, 2),
                advance_to(5, TurnStep::Upkeep),
                Resolve,
                ME.has(Zone::Graveyard, Card::Forest, 2),
                ME.has(Zone::Graveyard, Card::Island, 1),
                ME.has(Zone::Hand, Card::Mountain, 2),
                THEM.has_cards(Zone::Hand, 2),
                ME.has_cards(Zone::Hand, 2),
            ]),
        Scenario::new("One card in hand prevents the Case from triggering")
            .battlefield(P0, &[CASE])
            .hand(P0, &[Card::Mountain])
            .library(P0, &[Card::Forest; 5])
            .library(P1, &[Card::Island; 4])
            .actions(&[
                advance_to(1, TurnStep::End),
                stack(0),
                advance_to(3, TurnStep::Upkeep),
                stack(0),
                ME.has(Zone::Hand, Card::Mountain, 1),
            ]),
        Scenario::new("Drawing in response prevents the Case from solving")
            .interaction(&[Card::CerebralVortex])
            .battlefield(P0, &[CASE])
            .battlefield(P1, &[Card::Island, Card::Mountain, Card::Island])
            .hand(P1, &[Card::CerebralVortex])
            .library(P0, &[Card::Forest; 8])
            .library(P1, &[Card::Island; 4])
            .actions(&[
                advance_to(1, TurnStep::End),
                stack(1),
                ME.passes(),
                THEM.casts(Card::CerebralVortex),
                THEM.targets_player(ME),
                Resolve,
                advance_to(3, TurnStep::Upkeep),
                stack(0),
                ME.has_cards(Zone::Hand, 2),
            ]),
        Scenario::new("A returned Case enters unsolved")
            .interaction(&[WISP])
            .battlefield(P0, &[CASE])
            .battlefield(P1, &[Card::Plains; 3])
            .hand(P1, &[WISP])
            .library(P0, &[Card::Forest; 5])
            .library(P1, &[Card::Island; 4])
            .actions(&[
                advance_to(1, TurnStep::End),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(WISP),
                Resolve,
                THEM.targets(CASE),
                Resolve,
                ME.has(Zone::Exile, CASE, 1),
                advance_to(2, TurnStep::End),
                Resolve,
                ME.controls(CASE, 1),
                Resolve,
                ME.has_cards(Zone::Hand, 2),
                advance_to(3, TurnStep::Upkeep),
                stack(0),
            ]),
        Scenario::new("A copy of a solved Case is unsolved")
            .interaction(&[COPY])
            .battlefield(P0, &[CASE])
            .battlefield(P1, &[Card::Island; 4])
            .hand(P1, &[COPY, Card::Mountain])
            .library(P0, &[Card::Forest; 6])
            .library(P1, &[Card::Island; 6])
            .actions(&[
                advance_to(1, TurnStep::End),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(COPY),
                Resolve,
                THEM.picks_up_to(CASE),
                Resolve,
                THEM.picks(Card::Mountain),
                THEM.controls(CASE, 1),
                advance_to(3, TurnStep::Upkeep),
                Resolve,
                advance_to(4, TurnStep::Upkeep),
                stack(0),
            ]),
    ];
}
