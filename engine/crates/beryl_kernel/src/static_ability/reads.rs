//! What a condition's amounts read: counters, and whether permanents can share an answer.
//!
//! Implements: none. Reads static ability and state trigger conditions.

use beryl_card::{Condition, CounterKind};

use super::{Counted, compares, watched};

/// Whether every permanent with the same card, ability and controller gets the same answer to
/// this state trigger's `condition`: it is [`watched`] and compares no counters, so it reads
/// only permanents through a filter, a life total or a hand size, all for the controller.
pub fn shared(condition: &Condition) -> bool {
    watched(condition) && !compares(condition, &|n| matches!(n, Counted::Counters(_)))
}

/// Whether a static ability's "as long as" reads the number of `kind` counters on the
/// permanent with the ability, so that a change in it may change whether the ability
/// applies.
pub fn reads_counters(condition: &Condition, kind: CounterKind) -> bool {
    compares(
        condition,
        &|n| matches!(n, Counted::Counters(k) if *k == kind),
    )
}
