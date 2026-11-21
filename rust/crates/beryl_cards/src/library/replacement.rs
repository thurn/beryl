//! Replacement effects ([CR 614](https://yawgatog.com/resources/magic-rules/#R614)).

use beryl_engine::state::CounterKind;
use beryl_scenario::{P0, Scenario};
use beryl_scenario_types::ME;

use crate::names;

/// Effects that say a permanent "enters with" counters are replacement effects ([CR 614.1c]).
///
/// [CR 614.1c]: https://yawgatog.com/resources/magic-rules/#R6141c
pub const ENTERS_WITH_COUNTERS: &[Scenario] = &[Scenario::new("it enters with ten ice counters")
    .hand(P0, &[names::DARK_DEPTHS])
    .actions(&[
        ME.plays(names::DARK_DEPTHS),
        ME.counters(names::DARK_DEPTHS, CounterKind::Ice, 10),
    ])];
