//! The Breach loop (Underworld Breach, Lion's Eye Diamond and Brain Freeze), played by hand
//! with no shortcut, fits in one turn's decision budget: the budget that stops a runaway line
//! in search and rollouts would otherwise cut a legal win short.

use beryl_engine::Limits;

use super::scenarios_of;
use crate::{CARDS, names};

/// The most decisions the turn of the loop reaches, which must stay below the budget with
/// room for the other player's priorities and a slightly longer line.
#[test]
fn the_breach_loop_fits_the_turns_decision_budget() {
    let scenario = scenarios_of(names::UNDERWORLD_BREACH)
        .iter()
        .find(|s| s.name == "the Breach loop empties a 53-card library")
        .expect("Underworld Breach's loop scenario");
    let mut most = 0;
    beryl_scenario::watch(&CARDS, scenario, |state, _| {
        most = most.max(state.budgets().decisions);
    })
    .unwrap_or_else(|failure| panic!("{failure:?}"));
    let budget = Limits::DEFAULT.decisions_per_turn;
    assert!(
        most < budget / 2,
        "the loop took {most} of the turn's {budget} decisions"
    );
}
