//! Casting spells ([CR 601](https://yawgatog.com/resources/magic-rules/#R601)) as an effect
//! allows.

use beryl_engine::state::CounterKind;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::names;

const MALCOLM: &str = names::MALCOLM_ALLURING_SCOUNDREL;

/// The creature attacks alone in `turn` and its trigger resolves.
macro_rules! hit {
    ($turn:expr) => {
        [
            Action::AdvanceTo {
                turn: $turn,
                step: TurnStep::BeginningOfCombat,
            },
            Action::pass(P0),
            Action::choose(P0, DecisionKind::Attacker, MALCOLM),
            Action::AdvanceTo {
                turn: $turn,
                step: TurnStep::CombatDamage,
            },
            Action::Resolve,
        ]
    };
}

const HITS: [[Action; 5]; 4] = [hit!(1), hit!(3), hit!(5), hit!(7)];

/// A card an effect lets a player cast is one they may cast
/// ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)): "If an effect
/// specifically instructs or allows a player to cast a spell during resolution, they do so
/// by following the steps in rules 601.2a-i". A land is no spell to cast
/// ([CR 305.9](https://yawgatog.com/resources/magic-rules/#R3059)), so with a land the card
/// discarded on the fourth hit nothing is offered and it stays in the graveyard.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("An effect that lets a player cast a card offers no land")
            .battlefield(P0, &[Permanent::new(MALCOLM)])
            .library(P0, &[names::ISLAND; 7])
            .library(P1, &[names::PLAINS, names::PLAINS, names::PLAINS])
            .actions(&[
                HITS[0][0],
                HITS[0][1],
                HITS[0][2],
                HITS[0][3],
                HITS[0][4],
                HITS[1][0],
                HITS[1][1],
                HITS[1][2],
                HITS[1][3],
                HITS[1][4],
                HITS[2][0],
                HITS[2][1],
                HITS[2][2],
                HITS[2][3],
                HITS[2][4],
                HITS[3][0],
                HITS[3][1],
                HITS[3][2],
                HITS[3][3],
                HITS[3][4],
                Action::assert(Assertion::Counters {
                    player: P0,
                    card: MALCOLM,
                    kind: CounterKind::Chorus,
                    count: 4,
                }),
                Action::assert(Assertion::Stack { count: 0 }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(names::ISLAND),
                    count: 4,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Hand,
                    card: Some(names::ISLAND),
                    count: 3,
                }),
            ]),
    ];
