// Generated from the card store by `just card header`: notes go below the end marker.
//! Hullbreacher {2}{U}
//! Creature — Merfolk Pirate, 3/2
//!
//! Flash
//! If an opponent would draw a card except the first one they draw in each of their draw steps,
//! instead you create a Treasure token. (It's an artifact with "{T}, Sacrifice this token: Add one
//! mana of any color.")
// End of the generated card header.

use crate::prelude::*;
use crate::tokens::TREASURE;

/// Two Hullbreachers under the same player's control create a single Treasure for one
/// draw. Once the first replaces the draw, the event no longer matches the second.
/// [CR 614.6] says “If an event is replaced, it never happens.”
///
/// [CR 614.6]: https://yawgatog.com/resources/magic-rules/#R6146
pub static CARD: CardDef = Printed::Hullbreacher
    .keywords(Keywords::FLASH)
    .abilities(&[
        Ability::replacing(&Replacement::draws_except_first_in_draw_step(
            Players::EachOpponent,
        ))
        .effect(&create_tokens(you(), &TREASURE, 1)),
    ])
    .card()
    .creating(&[&TREASURE]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;
    use crate::tokens::TREASURE_TOKEN;

    const HULLBREACHER: &str = Card::Hullbreacher;
    const JACE: &str = Card::JaceVrynsProdigy;

    pub const SCENARIOS: &[Scenario] = &[
        // The opponent's loot makes P0 a Treasure instead of drawing, and the opponent still
        // discards.
        Scenario::new("an opponent's draw is a Treasure for P0 instead")
            .interaction(&[JACE])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[HULLBREACHER])
            .battlefield(P1, &[JACE])
            .hand(P1, &[Card::Island])
            .library(P0, &[Card::Plains, Card::Island])
            .library(P1, &[Card::Counterspell, Card::Island])
            .actions(&[
                THEM.activates(JACE, 0), // {T}: draw, then discard
                Resolve,
                ME.controls(TREASURE_TOKEN, 1),
                ME.has_cards(Zone::Hand, 0),
                THEM.has_cards(Zone::Hand, 0),
                THEM.has(Zone::Graveyard, Card::Island, 1),
                THEM.library_top(&[Card::Counterspell]),
            ]),
        // The draw step's draw is the opponent's own; a later draw in the same step is not.
        Scenario::new("the opponent's draw step's first draw is theirs, the next is not")
            .interaction(&[JACE])
            .at(2, P1, TurnStep::Upkeep)
            .battlefield(P0, &[HULLBREACHER])
            .battlefield(P1, &[JACE])
            .library(P0, &[Card::Plains, Card::Island])
            .library(P1, &[Card::Counterspell, Card::Island])
            .actions(&[
                THEM.passes(),
                THEM.has(Zone::Hand, Card::Counterspell, 1),
                ME.controls(TREASURE_TOKEN, 0),
                THEM.activates(JACE, 0), // {T}: draw, then discard
                Resolve,
                ME.controls(TREASURE_TOKEN, 1),
                THEM.has_cards(Zone::Hand, 0),
                THEM.has(Zone::Graveyard, Card::Counterspell, 1),
                THEM.library_top(&[Card::Island]),
            ]),
        // Its controller's own draws are not replaced.
        Scenario::new("its controller's own draw is not replaced")
            .interaction(&[JACE])
            .battlefield(P0, &[HULLBREACHER, JACE])
            .library(P0, &[Card::Plains, Card::Island])
            .library(P1, &[Card::Counterspell, Card::Island])
            .actions(&[
                ME.activates(JACE, 0), // {T}: draw, then discard
                Resolve,
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.library_top(&[Card::Island]),
                ME.controls(TREASURE_TOKEN, 0),
            ]),
    ];
}
