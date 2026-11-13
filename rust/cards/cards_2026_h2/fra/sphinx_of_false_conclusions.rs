// Generated from the card store by `just card header`: notes go below the end marker.
//! Sphinx of False Conclusions {2}{U}{U}
//! Creature — Sphinx Illusion, 4/2
//!
//! Flash
//! Flying
//! Whenever this creature attacks, draw a card, then discard a card.
//! When this creature dies, if it isn't a token, create a token that's a copy of it.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::SphinxOfFalseConclusions
    .keywords(Keywords::FLASH.union(Keywords::FLYING))
    .abilities(&[
        // "Draw a card, then discard a card."
        Ability::triggered(&Trigger::ATTACKS)
            .effect(&sequence(&[draw(you(), 1), discard(you(), 1)])),
        // "If it isn't a token", as it last existed on the battlefield
        // ([CR 603.10a](https://yawgatog.com/resources/magic-rules/#R60310a)): "Some
        // zone-change triggers look back in time." The copy's copiable values are its
        // card's ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)).
        Ability::triggered(&Trigger::DIES)
            .only_if(&Condition::Not(&Condition::Token(this())))
            .effect(&create_copy(you(), this())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SPHINX: &str = Card::SphinxOfFalseConclusions;

    /// The Sphinx attacks in `turn`, loots, and a Spellbinder blocks it: both die.
    macro_rules! traded {
        ($turn:expr) => {
            [
                advance_to($turn, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(SPHINX),
                Resolve,
                ME.passes(),
                THEM.chooses(DecisionKind::Blocker, Card::EliteSpellbinder),
            ]
        };
    }

    const TRADES: [[Action; 6]; 2] = [traded!(1), traded!(3)];

    pub const SCENARIOS: &[Scenario] = &[
        // The card dies and leaves a token copy behind; the token dies and leaves nothing.
        Scenario::new("Sphinx dies as a card, then as a token")
            .battlefield(P0, &[SPHINX])
            .battlefield(P1, &[Card::EliteSpellbinder, Card::EliteSpellbinder])
            .library(P0, &[Card::Island, Card::Island, Card::Island])
            .library(P1, &[Card::Plains])
            .actions(&[
                TRADES[0][0],
                TRADES[0][1],
                TRADES[0][2],
                TRADES[0][3],
                ME.has(Zone::Graveyard, Card::Island, 1),
                TRADES[0][4],
                TRADES[0][5],
                // The other Spellbinder stays back.
                THEM.answers(DecisionKind::Blocker, Pick::Done),
                advance_to(1, TurnStep::CombatDamage),
                Resolve,
                ME.token_pt(SPHINX, 4, 2),
                ME.has(Zone::Graveyard, SPHINX, 1),
                // The other Spellbinder stays home in its controller's turn.
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.done_attacking(),
                TRADES[1][0],
                TRADES[1][1],
                TRADES[1][2],
                TRADES[1][3],
                TRADES[1][4],
                TRADES[1][5],
                advance_to(3, TurnStep::EndOfCombat),
                ME.controls(SPHINX, 0),
                THEM.has(Zone::Graveyard, Card::EliteSpellbinder, 2),
                THEM.life(20),
            ]),
    ];
}
