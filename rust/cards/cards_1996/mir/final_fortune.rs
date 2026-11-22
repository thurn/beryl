// Generated from the card store by `just card header`: notes go below the end marker.
//! Final Fortune {R}{R}
//! Instant
//!
//! Take an extra turn after this one. At the beginning of that turn's end step, you lose the game.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::FinalFortune
    .abilities(&[
        Ability::spell().effect(&extra_turn_with_trigger(you(), 1)),
        Ability::triggered(&Trigger::during_extra_turn(TurnStep::End)).effect(&lose_game(you())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const FORTUNE: &str = Card::FinalFortune;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Final Fortune loses the game in the extra turn's end step")
            .hand(P0, &[FORTUNE])
            .battlefield(P0, &[Card::Mountain; 2])
            .actions(&[
                ME.casts(FORTUNE),
                Resolve,
                advance_to(1, TurnStep::End),
                stack(0),
                advance_to(2, TurnStep::PrecombatMain),
                ME.has_cards(Zone::Hand, 1),
                advance_to(2, TurnStep::End),
                stack(1),
                Resolve,
                wins(THEM),
            ]),
        Scenario::new("Skipping Final Fortune's extra turn prevents its loss trigger")
            .interaction(&[Card::TimeVault])
            .hand(P0, &[Card::TimeVault, FORTUNE])
            .battlefield(P0, &[Card::Mountain; 4])
            .actions(&[
                ME.casts(Card::TimeVault),
                Resolve,
                ME.casts(FORTUNE),
                Resolve,
                advance_to(1, TurnStep::End),
                ME.passes(),
                ME.picks_up_to(Card::TimeVault),
                advance_to(2, TurnStep::PrecombatMain),
                ME.tapped(Card::TimeVault, 0),
                ME.has_cards(Zone::Hand, 0),
                THEM.has_cards(Zone::Hand, 1),
                advance_to(3, TurnStep::End),
                stack(0),
            ]),
        Scenario::new("An extra turn granted later goes before Final Fortune's turn")
            .interaction(&[Card::TimeWarp])
            .hand(P0, &[FORTUNE, Card::TimeWarp])
            .battlefield(
                P0,
                &[
                    Card::Mountain,
                    Card::Mountain,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .actions(&[
                ME.casts(FORTUNE),
                Resolve,
                ME.casts(Card::TimeWarp),
                ME.targets_player(THEM),
                Resolve,
                advance_to(2, TurnStep::End),
                ME.has_cards(Zone::Hand, 0),
                THEM.has_cards(Zone::Hand, 1),
                stack(0),
                advance_to(3, TurnStep::End),
                stack(1),
                Resolve,
                wins(THEM),
            ]),
        Scenario::new("Final Fortune's caster loses even if Mindslaver controls the turn")
            .interaction(&[Card::Mindslaver])
            .battlefield(
                P0,
                &[
                    Card::Mindslaver,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P1, &[FORTUNE])
            .battlefield(P1, &[Card::Mountain; 2])
            .library(P1, &[Card::Forest])
            .actions(&[
                ME.activates(Card::Mindslaver, 0),
                ME.targets_player(THEM),
                Resolve,
                advance_to(1, TurnStep::End),
                ME.passes(),
                THEM.casts(FORTUNE),
                advance_to(2, TurnStep::PrecombatMain),
                ME.plays(Card::Forest),
                THEM.controls(Card::Forest, 1),
                advance_to(2, TurnStep::End),
                stack(1),
                Resolve,
                wins(ME),
            ]),
        Scenario::new("Sundial exiles Final Fortune's loss trigger")
            .interaction(&[Card::SundialOfTheInfinite])
            .hand(P0, &[FORTUNE])
            .battlefield(
                P0,
                &[Card::SundialOfTheInfinite, Card::Mountain, Card::Mountain],
            )
            .actions(&[
                ME.casts(FORTUNE),
                Resolve,
                advance_to(2, TurnStep::End),
                stack(1),
                ME.activates(Card::SundialOfTheInfinite, 0),
                Resolve,
                advance_to(3, TurnStep::End),
                stack(0),
            ]),
        Scenario::new("Time Stop can end the extra turn before Final Fortune triggers")
            .interaction(&[Card::TimeStop])
            .hand(P0, &[FORTUNE, Card::TimeStop])
            .battlefield(
                P0,
                &[
                    Card::Mountain,
                    Card::Mountain,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .actions(&[
                ME.casts(FORTUNE),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                ME.casts(Card::TimeStop),
                Resolve,
                ME.has(Zone::Exile, Card::TimeStop, 1),
                advance_to(3, TurnStep::End),
                stack(0),
            ]),
    ];
}
