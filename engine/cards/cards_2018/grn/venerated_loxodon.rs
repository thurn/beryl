// Generated from the card store by `just card header`: notes go below the end marker.
//! Venerated Loxodon {4}{W}
//! Creature — Elephant Cleric, 4/4
//!
//! Convoke (Your creatures can help cast this spell. Each creature you tap while casting this spell
//! pays for {1} or one mana of that creature's color.)
//! When this creature enters, put a +1/+1 counter on each creature that convoked it.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::VeneratedLoxodon
    .keywords(Keywords::CONVOKE)
    .abilities(&[when_enters(&add_counters(
        Objects::Convoked,
        CounterKind::PlusOne,
        1,
    ))])
    .card();
