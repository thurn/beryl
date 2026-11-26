// Generated from the card store by `just card header`: notes go below the end marker.
//! Accumulated Knowledge {1}{U}
//! Instant
//!
//! Draw a card, then draw cards equal to the number of cards named Accumulated Knowledge in all
//! graveyards.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::AccumulatedKnowledge
    .abilities(&[Ability::spell().effect(&sequence(&[
        draw(you(), 1),
        // Counted when this instruction is reached, with this spell still on the stack, so it
        // is not among the cards counted.
        Effect::Draw {
            player: you(),
            count: Amount::Count(&Objects::All {
                zone: Zone::Graveyard,
                filter: Filter::Named("Accumulated Knowledge"),
            }),
        },
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const KNOWLEDGE: &str = Card::AccumulatedKnowledge;

    pub const SCENARIOS: &[Scenario] = &[
        // One card, then one for each copy in any graveyard: one in each player's graveyard
        // makes three cards. The spell on the stack is in no graveyard, so it adds none, and
        // the opponent draws nothing.
        Scenario::new("draws a card plus one for each copy in every graveyard")
            .hand(P0, &[KNOWLEDGE])
            .battlefield(P0, &[Card::Island, Card::Island])
            .graveyard(P0, &[KNOWLEDGE])
            .graveyard(P1, &[KNOWLEDGE])
            .library(
                P0,
                &[Card::Plains, Card::Plains, Card::Plains, Card::Plains],
            )
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.casts(KNOWLEDGE),
                Resolve,
                ME.has_cards(Zone::Hand, 3),
                THEM.has_cards(Zone::Hand, 0),
                ME.has(Zone::Graveyard, KNOWLEDGE, 2),
            ]),
        Scenario::new("Accumulated Knowledge counts copies in the shared graveyard once")
            .shared_library(&[Card::Island; 4])
            .shared_graveyard(&[KNOWLEDGE, KNOWLEDGE])
            .hand(P0, &[KNOWLEDGE])
            .battlefield(P0, &[Card::Island, Card::Island])
            .actions(&[
                ME.casts(KNOWLEDGE),
                Resolve,
                ME.has_cards(Zone::Hand, 3),
                ME.has(Zone::Graveyard, KNOWLEDGE, 3),
                THEM.has(Zone::Graveyard, KNOWLEDGE, 3),
            ]),
    ];
}
