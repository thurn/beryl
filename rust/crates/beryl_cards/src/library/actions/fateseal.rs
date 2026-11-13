//! Fateseal ([CR 701.29](https://yawgatog.com/resources/magic-rules/#R70129)).

use beryl_engine::DecisionKind;
use beryl_scenario::{Action, Assertion, Ending, P0, P1, Permanent, Pick, Scenario};

use crate::names;

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
