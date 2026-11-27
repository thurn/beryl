// Generated from the card store by `just card header`: notes go below the end marker.
//! Kalonian Hydra {3}{G}{G}
//! Creature — Hydra, 0/0
//!
//! Trample
//! This creature enters with four +1/+1 counters on it.
//! Whenever this creature attacks, double the number of +1/+1 counters on each creature you
//! control.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::KalonianHydra
    .keywords(Keywords::TRAMPLE)
    .abilities(&[
        Ability::replacing(&Replacement::enters_with_counters(CounterKind::PlusOne, 4)),
        whenever_attacks(&double_counters(
            permanents(creature_you_control()),
            CounterKind::PlusOne,
        )),
    ])
    .card();
