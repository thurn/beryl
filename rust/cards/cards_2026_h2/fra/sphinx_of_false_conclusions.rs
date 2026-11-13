// Generated from the card store by `just card header`: notes go below the end marker.
//! Sphinx of False Conclusions {2}{U}{U}
//! Creature — Sphinx Illusion, 4/2
//!
//! Flash
//! Flying
//! Whenever this creature attacks, draw a card, then discard a card.
//! When this creature dies, if it isn't a token, create a token that's a copy of it.
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{THIS, YOU, create_copy, discard, draw, sequence};
use beryl_card::{Ability, CardDef, Condition, Keywords, Trigger, card};

pub static CARD: CardDef = card(
    names::SPHINX_OF_FALSE_CONCLUSIONS,
    &[printed::SPHINX_OF_FALSE_CONCLUSIONS
        .keywords(Keywords::FLASH.union(Keywords::FLYING))
        .abilities(&[
            // "Draw a card, then discard a card."
            Ability::triggered(&Trigger::ATTACKS)
                .effect(&sequence(&[draw(YOU, 1), discard(YOU, 1)])),
            // "If it isn't a token", as it last existed on the battlefield
            // ([CR 603.10a](https://yawgatog.com/resources/magic-rules/#R60310a)): "Some
            // zone-change triggers look back in time." The copy's copiable values are its
            // card's ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)).
            Ability::triggered(&Trigger::DIES)
                .only_if(&Condition::Not(&Condition::Token(THIS)))
                .effect(&create_copy(YOU, THIS)),
        ])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::{DecisionKind, TurnStep, Zone};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::names;

    const SPHINX: &str = names::SPHINX_OF_FALSE_CONCLUSIONS;

    const SPELLBINDER: &str = names::ELITE_SPELLBINDER;

    /// The Sphinx attacks in `turn`, loots, and a Spellbinder blocks it: both die.
    macro_rules! traded {
        ($turn:expr) => {
            [
                Action::AdvanceTo {
                    turn: $turn,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Priority,
                    pick: Pick::Pass,
                },
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Attacker,
                    pick: Pick::Card { card: SPHINX },
                },
                Action::Resolve,
                Action::Answer {
                    player: P0,
                    kind: DecisionKind::Priority,
                    pick: Pick::Pass,
                },
                Action::Answer {
                    player: P1,
                    kind: DecisionKind::Blocker,
                    pick: Pick::Card { card: SPELLBINDER },
                },
            ]
        };
    }

    const TRADES: [[Action; 6]; 2] = [traded!(1), traded!(3)];

    pub const SCENARIOS: &[Scenario] = &[
        // The card dies and leaves a token copy behind; the token dies and leaves nothing.
        Scenario::new("Sphinx dies as a card, then as a token")
            .permanents(P0, &[Permanent::new(SPHINX)])
            .permanents(
                P1,
                &[Permanent::new(SPELLBINDER), Permanent::new(SPELLBINDER)],
            )
            .library(P0, &[names::ISLAND, names::ISLAND, names::ISLAND])
            .library(P1, &[names::PLAINS])
            .actions(&[
                TRADES[0][0],
                TRADES[0][1],
                TRADES[0][2],
                TRADES[0][3],
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P0,
                        zone: Zone::Graveyard,
                        card: Some(names::ISLAND),
                        count: 1,
                    },
                },
                TRADES[0][4],
                TRADES[0][5],
                // The other Spellbinder stays back.
                Action::Answer {
                    player: P1,
                    kind: DecisionKind::Blocker,
                    pick: Pick::Done,
                },
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::CombatDamage,
                },
                Action::Resolve,
                Action::Assert {
                    assertion: Assertion::Pt {
                        player: P0,
                        card: SPHINX,
                        token: true,
                        power: 4,
                        toughness: 2,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P0,
                        zone: Zone::Graveyard,
                        card: Some(SPHINX),
                        count: 1,
                    },
                },
                // The other Spellbinder stays home in its controller's turn.
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::Answer {
                    player: P1,
                    kind: DecisionKind::Priority,
                    pick: Pick::Pass,
                },
                Action::Answer {
                    player: P1,
                    kind: DecisionKind::Attacker,
                    pick: Pick::Done,
                },
                TRADES[1][0],
                TRADES[1][1],
                TRADES[1][2],
                TRADES[1][3],
                TRADES[1][4],
                TRADES[1][5],
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::EndOfCombat,
                },
                Action::Assert {
                    assertion: Assertion::Controls {
                        player: P0,
                        card: SPHINX,
                        count: 0,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Count {
                        player: P1,
                        zone: Zone::Graveyard,
                        card: Some(SPELLBINDER),
                        count: 2,
                    },
                },
                Action::Assert {
                    assertion: Assertion::Life {
                        player: P1,
                        life: 20,
                    },
                },
            ]),
    ];
}
