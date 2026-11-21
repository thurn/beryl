//! State-based actions ([CR 704]).
//!
//! [CR 704]: https://yawgatog.com/resources/magic-rules/#R704

use beryl_engine::TurnStep;
use beryl_scenario::{P0, P1, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to, wins};

use crate::{names, props};

const BOLT: &str = names::LIGHTNING_BOLT;
const BOLT_SETUP: Scenario = Scenario::setup()
    .hand(P0, &[BOLT])
    .battlefield(P0, &[names::MOUNTAIN]);

/// A player loses when their life reaches zero or falls below it.
pub const ZERO_LIFE: &[Scenario] = &[
    BOLT_SETUP
        .titled("a player at zero life loses during their own turn")
        .life(P0, 3)
        .actions(&[ME.casts(BOLT), ME.targets_player(ME), Resolve, wins(THEM)]),
    BOLT_SETUP
        .titled("a player at zero life loses during the opponent's turn")
        .life(P1, 3)
        .actions(&[ME.casts(BOLT), ME.targets_player(THEM), Resolve, wins(ME)]),
    Scenario::new("combat damage can reduce the losing player's life below zero")
        .life(P1, 1)
        .battlefield(P0, &[props::VANILLA.card])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(props::VANILLA.card),
            wins(ME),
        ]),
];
