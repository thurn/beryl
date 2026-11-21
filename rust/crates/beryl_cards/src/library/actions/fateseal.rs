//! Fateseal ([CR 701.29](https://yawgatog.com/resources/magic-rules/#R70129)).

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, Ending, P0, P1, Permanent, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to};

use crate::{names, props};

const JACE: &str = names::JACE_THE_MIND_SCULPTOR;

/// With no card on top of the library there is none to look at and nothing to ask
/// ([CR 609.3](https://yawgatog.com/resources/magic-rules/#R6093)): "If an effect attempts to
/// do something impossible, it does only as much as possible." The ability resolves with no
/// question, and the player whose library is empty loses at their next draw
/// ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)).
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("Looking at the top card of an empty library asks nothing")
            .permanents(P0, &[Permanent::new(JACE)])
            .library(P0, &[names::ISLAND, names::ISLAND])
            .library(P1, &[])
            .actions(&[
                Action::activate(P0, JACE, 0),
                Action::answer(P0, DecisionKind::Target, Pick::Player { player: P1 }),
                Action::assert(Assertion::Outcome {
                    outcome: Ending::Win { player: P0 },
                }),
            ]),
    ];

/// A player can put the looked-at card on the bottom or leave it in place.
pub const TOP_OR_BOTTOM: &[Scenario] =
    &[
        Scenario::new("+2 puts the top of an opponent's library on the bottom")
            .battlefield(P0, &[JACE])
            .library(P0, &[names::ISLAND, names::PLAINS])
            .library(P1, &[props::ARTIFACT.card, names::PLAINS, names::ISLAND])
            .actions(&[
                ME.activates(JACE, 0), // +2
                ME.targets_player(THEM),
                Resolve,
                ME.yes(),
                // Assertions read the next open decision: by then, the opponent has drawn the
                // Plains that the artifact left on top.
                THEM.library_top(&[names::ISLAND, props::ARTIFACT.card]),
                THEM.has(Zone::Hand, names::PLAINS, 1),
                ME.has_loyalty(JACE, 5),
                // The next turn, Jace looks at his own top card and keeps it there.
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(JACE, 0),
                ME.targets_player(ME),
                ME.no(),
                ME.library_top(&[names::PLAINS]),
                ME.has_loyalty(JACE, 7),
            ]),
    ];
