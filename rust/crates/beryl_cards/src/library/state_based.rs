//! State-based actions ([CR 704]).
//!
//! [CR 704]: https://yawgatog.com/resources/magic-rules/#R704

use beryl_engine::TurnStep;
use beryl_scenario::{P0, P1, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to, ends_in_draw, wins};

use crate::{names, props};

const HARPY: &str = names::BLOOD_TOLL_HARPY;
const HARPY_SETUP: Scenario = Scenario::setup()
    .hand(P0, &[HARPY])
    .battlefield(P0, &[names::SWAMP; 3]);

/// A player with zero life loses, and if both players reach zero simultaneously the game
/// ends in a draw.
pub const ZERO_LIFE: &[Scenario] = &[
    HARPY_SETUP
        .titled("losing life to zero on your own turn loses the game")
        .life(P0, 1)
        .actions(&[ME.casts(HARPY), Resolve, Resolve, wins(THEM)]),
    HARPY_SETUP
        .titled("losing life to zero on the opponent's turn loses the game")
        .life(P1, 1)
        .actions(&[ME.casts(HARPY), Resolve, Resolve, wins(ME)]),
    HARPY_SETUP
        .titled("both players reaching zero life at once draws the game")
        .life(P0, 1)
        .life(P1, 1)
        .actions(&[ME.casts(HARPY), Resolve, Resolve, ends_in_draw()]),
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
