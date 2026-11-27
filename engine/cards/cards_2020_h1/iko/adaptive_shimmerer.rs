// Generated from the card store by `just card header`: notes go below the end marker.
//! Adaptive Shimmerer {5}
//! Creature — Insect, 0/0
//!
//! Flash
//! This creature enters with three +1/+1 counters on it.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::AdaptiveShimmerer
    .keywords(Keywords::FLASH)
    .abilities(&[Ability::replacing(&Replacement::enters_with_counters(
        CounterKind::PlusOne,
        3,
    ))])
    .card();
