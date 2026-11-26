// Generated from the card store by `just card header`: notes go below the end marker.
//! Capture of Jingzhou {3}{U}{U}
//! Sorcery
//!
//! Take an extra turn after this one.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::CaptureOfJingzhou
    .abilities(&[Ability::spell().effect(&extra_turn(you()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // The caster takes the turn after this one, so the opponent's turn follows it.
        Scenario::new("the caster takes the next turn, then the opponent")
            .at(1, P0, TurnStep::PrecombatMain)
            .hand(P0, &[Card::CaptureOfJingzhou])
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .library(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .library(P1, &[Card::Plains, Card::Plains, Card::Plains])
            .actions(&[
                ME.casts(Card::CaptureOfJingzhou),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                ME.has_cards(Zone::Hand, 1),
                THEM.has_cards(Zone::Hand, 0),
                advance_to(3, TurnStep::PrecombatMain),
                ME.has_cards(Zone::Hand, 1),
                THEM.has_cards(Zone::Hand, 1),
            ]),
        Scenario::new("Capture of Jingzhou's extra turn draws from the shared library")
            .at(1, P0, TurnStep::PrecombatMain)
            .shared_library(&[Card::Plains, Card::Island])
            .hand(P0, &[Card::CaptureOfJingzhou])
            .battlefield(P0, &[Card::Island; 5])
            .actions(&[
                ME.casts(Card::CaptureOfJingzhou),
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                ME.has(Zone::Hand, Card::Plains, 1),
                THEM.has_cards(Zone::Hand, 0),
                ME.has_cards(Zone::Library, 1),
            ]),
    ];
}
