// Generated from the card store by `just card header`: notes go below the end marker.
//! Notion Thief {2}{U}{B}
//! Creature — Human Rogue, 3/1
//!
//! Flash
//! If an opponent would draw a card except the first one they draw in each of their draw steps,
//! instead that player skips that draw and you draw a card.
// End of the generated card header.

use crate::prelude::*;

/// With a Notion Thief on each side, the first replacement gives the draw to the other
/// Thief's controller. The second sends it back. Under [CR 614.5], a replacement effect
/// “gets only one opportunity to affect an event or any modified events that may replace
/// that event.” With Chains of Mephistopheles, [CR 616.1] says “the affected player chooses
/// one to apply.”
///
/// [CR 614.5]: https://yawgatog.com/resources/magic-rules/#R6145
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
pub static CARD: CardDef = Printed::NotionThief
    .keywords(Keywords::FLASH)
    .abilities(&[
        Ability::replacing(&Replacement::draws_except_first_in_draw_step(
            Players::EachOpponent,
        ))
        .effect(&draw(you(), 1)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const THIEF: &str = Card::NotionThief;
    const JACE: &str = Card::JaceVrynsProdigy;
    const CHAINS: &str = Card::ChainsOfMephistopheles;

    // In the scenarios with a choice, P0's own Jace is something P0 could do after the loot
    // resolves, so the game stops there and not at P0's next turn.

    pub const SCENARIOS: &[Scenario] = &[
        // The opponent's loot draws for P0 instead, and the opponent still discards.
        Scenario::new("an opponent's draw is P0's instead")
            .interaction(&[JACE])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[THIEF])
            .battlefield(P1, &[JACE])
            .hand(P1, &[Card::Island])
            .library(P0, &[Card::Plains, Card::Island])
            .library(P1, &[Card::Counterspell, Card::Island])
            .actions(&[
                THEM.activates(JACE, 0), // {T}: draw, then discard
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 1),
                THEM.has_cards(Zone::Hand, 0),
                THEM.has(Zone::Graveyard, Card::Island, 1),
                THEM.library_top(&[Card::Counterspell]),
            ]),
        // The draw step's draw is the opponent's own; a later draw in the same step is not.
        Scenario::new("the opponent's draw step's first draw is theirs, the next is P0's")
            .interaction(&[JACE])
            .at(2, P1, TurnStep::Upkeep)
            .battlefield(P0, &[THIEF])
            .battlefield(P1, &[JACE])
            .library(P0, &[Card::Plains, Card::Island])
            .library(P1, &[Card::Counterspell, Card::Island])
            .actions(&[
                THEM.passes(),
                THEM.has(Zone::Hand, Card::Counterspell, 1),
                THEM.activates(JACE, 0), // {T}: draw, then discard
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 1),
                THEM.has_cards(Zone::Hand, 0),
                THEM.has(Zone::Graveyard, Card::Counterspell, 1),
                THEM.library_top(&[Card::Island]),
            ]),
        // Its controller's own draws are not replaced.
        Scenario::new("its controller's own draw is not replaced")
            .interaction(&[JACE])
            .battlefield(P0, &[THIEF, JACE])
            .library(P0, &[Card::Plains, Card::Island])
            .library(P1, &[Card::Counterspell, Card::Island])
            .actions(&[
                ME.activates(JACE, 0), // {T}: draw, then discard
                Resolve,
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.library_top(&[Card::Island]),
                THEM.has_cards(Zone::Hand, 0),
            ]),
        // P0's Notion Thief has P0 draw instead; P1's then replaces P0's draw, and P1 draws
        // after all: neither applies twice.
        Scenario::new("with a Notion Thief each, the player who would draw draws")
            .interaction(&[JACE])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[THIEF])
            .battlefield(P1, &[THIEF, JACE])
            .library(P0, &[Card::Plains, Card::Island])
            .library(P1, &[Card::Counterspell, Card::Island])
            .actions(&[
                THEM.activates(JACE, 0), // {T}: draw, then discard
                Resolve,
                ME.has_cards(Zone::Hand, 0),
                ME.library_top(&[Card::Plains]),
                THEM.has(Zone::Graveyard, Card::Counterspell, 1),
                THEM.library_top(&[Card::Island]),
            ]),
        // Notion Thief first: P0 draws instead, and Chains replaces P0's draw: P0 discards
        // and draws. The opponent still discards for the loot.
        Scenario::new("Notion Thief before Chains of Mephistopheles")
            .interaction(&[JACE, CHAINS])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[THIEF, CHAINS, JACE])
            .battlefield(P1, &[JACE])
            .hand(P0, &[Card::Plains])
            .hand(P1, &[Card::Island])
            .library(P0, &[Card::Counterspell, Card::Island])
            .library(P1, &[Card::Plains, Card::Island])
            .actions(&[
                THEM.activates(JACE, 0), // {T}: draw, then discard
                Resolve,
                THEM.chooses(DecisionKind::ChooseObject, THIEF),
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has(Zone::Hand, Card::Counterspell, 1),
                ME.has_cards(Zone::Hand, 1),
                THEM.has_cards(Zone::Hand, 0),
                THEM.has(Zone::Graveyard, Card::Island, 1),
                THEM.library_top(&[Card::Plains]),
            ]),
        // Chains first: the opponent discards, and Notion Thief replaces the draw that
        // follows. Chains has applied, so P0 simply draws. The loot then has nothing left
        // to discard.
        Scenario::new("Chains of Mephistopheles before Notion Thief")
            .interaction(&[JACE, CHAINS])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[THIEF, CHAINS, JACE])
            .battlefield(P1, &[JACE])
            .hand(P0, &[Card::Plains])
            .hand(P1, &[Card::Island])
            .library(P0, &[Card::Counterspell, Card::Island])
            .library(P1, &[Card::Plains, Card::Island])
            .actions(&[
                THEM.activates(JACE, 0), // {T}: draw, then discard
                Resolve,
                THEM.chooses(DecisionKind::ChooseObject, CHAINS),
                ME.has_cards(Zone::Hand, 2),
                ME.has(Zone::Hand, Card::Counterspell, 1),
                ME.has_cards(Zone::Graveyard, 0),
                THEM.has_cards(Zone::Hand, 0),
                THEM.has(Zone::Graveyard, Card::Island, 1),
                THEM.library_top(&[Card::Plains]),
            ]),
    ];
}
