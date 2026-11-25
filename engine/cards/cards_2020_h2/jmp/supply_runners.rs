// Generated from the card store by `just card header`: notes go below the end marker.
//! Supply Runners {4}{W}
//! Creature — Dog, 2/2
//!
//! When this creature enters, put a +1/+1 counter on each other creature you control.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::SupplyRunners
    .abilities(&[when_enters(&add_counters(
        Objects::Other(&Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
        }),
        CounterKind::PlusOne,
        1,
    ))])
    .card();
