//! Turn structure ([CR 500](https://yawgatog.com/resources/magic-rules/#R500)).

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{P0, P1, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to};

use crate::{names, props};

/// "The affected player is controlled during the entire turn" ([CR 724.1]).
///
/// [CR 724.1]: https://yawgatog.com/resources/magic-rules/#R7241
pub const CONTROLLED_TURN: &[Scenario] = &[Scenario::new("the control ends with the turn")
    .at(1, P0, TurnStep::PostcombatMain)
    .battlefield(
        P0,
        &[
            names::MINDSLAVER,
            names::ISLAND,
            names::ISLAND,
            names::ISLAND,
            names::ISLAND,
            props::VANILLA.card,
        ],
    )
    .battlefield(P1, &[names::PLAINS, props::FLIER.card])
    .hand(P1, &[props::SORCERY.card])
    .library(P0, &[names::ISLAND, names::ISLAND])
    .library(P1, &[names::ISLAND, names::ISLAND])
    .actions(&[
        ME.activates(names::MINDSLAVER, 0),
        ME.targets_player(THEM),
        Resolve,
        advance_to(2, TurnStep::PrecombatMain),
        // Passing, playing P1's Island, or casting P1's Oust.
        ME.deciding(DecisionKind::Priority, 3),
        advance_to(2, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.done_attacking(),
        // In their own turn P0 decides for themself, as ever.
        advance_to(3, TurnStep::BeginningOfCombat),
        ME.passes(),
        ME.done_attacking(),
        advance_to(4, TurnStep::PrecombatMain),
        THEM.casts(props::SORCERY.card),
        THEM.targets(props::VANILLA.card),
        Resolve,
        ME.has(Zone::Library, props::VANILLA.card, 1),
        ME.life(23),
    ])];
