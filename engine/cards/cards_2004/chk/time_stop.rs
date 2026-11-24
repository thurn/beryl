// Generated from the card store by `just card header`: notes go below the end marker.
//! Time Stop {4}{U}{U}
//! Instant
//!
//! End the turn. (Exile all spells and abilities, including this spell. The player whose turn it is
//! discards down to their maximum hand size. Damage heals and "this turn" and "until end of turn"
//! effects end.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::TimeStop
    .abilities(&[Ability::spell().effect(&end_turn())])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const STOP: &str = Card::TimeStop;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Time Stop exiles itself and an uncounterable spell on the opponent's turn")
            .interaction(&[Card::CavernOfSouls, Card::MotherOfRunes])
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(P0, &[STOP])
            .battlefield(P0, &[Card::Island; 6])
            .hand(P1, &[Card::CavernOfSouls, Prop::Vanilla])
            .battlefield(P1, &[Card::Plains, Card::MotherOfRunes])
            .actions(&[
                THEM.plays(Card::CavernOfSouls),
                THEM.chooses_named("Elk"),
                THEM.casts(Prop::Vanilla),
                THEM.passes(),
                ME.casts(STOP),
                Resolve,
                stack(0),
                ME.has(Zone::Exile, STOP, 1),
                THEM.has(Zone::Exile, Prop::Vanilla, 1),
                THEM.controls(Prop::Vanilla, 0),
                advance_to(3, TurnStep::PrecombatMain),
            ]),
        Scenario::new("Time Stop gives a cleanup discard trigger a chance to resolve")
            .interaction(&[Card::CurrencyConverter])
            .hand(
                P0,
                &[
                    STOP,
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .battlefield(
                P0,
                &[
                    Card::CurrencyConverter,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .actions(&[
                ME.casts(STOP),
                Resolve,
                ME.picks(Card::Plains),
                Resolve,
                ME.yes(),
                ME.has(Zone::Exile, Card::Plains, 1),
                ME.has_cards(Zone::Hand, 7),
                advance_to(2, TurnStep::PrecombatMain),
            ]),
    ];
}
