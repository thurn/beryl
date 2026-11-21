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

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("another permanent entering leaves the stack empty")
                .battlefield(P0, &[Card::SupplyRunners, Prop::Vanilla])
                .hand(P0, &[Card::Island])
                .actions(&[ME.plays(Card::Island), stack(0)]),
        ];
}
