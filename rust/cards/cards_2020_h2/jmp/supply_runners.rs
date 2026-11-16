// Generated from the card store by `just card header`: notes go below the end marker.
//! Supply Runners {4}{W}
//! Creature — Dog, 2/2
//!
//! When this creature enters, put a +1/+1 counter on each other creature you control.
// End of the generated card header.

use crate::prelude::*;

/// "Each other creature you control" is every creature its controller controls except the
/// Dog itself ([CR 109.1]): it is "an object" like the others, and the instruction leaves
/// out the object whose ability this is. The set is found as the trigger resolves, so a
/// creature that entered after the ability went on the stack gets a counter too, and one that
/// left does not.
///
/// [CR 109.1]: https://yawgatog.com/resources/magic-rules/#R1091
pub static CARD: CardDef = Printed::SupplyRunners
    .abilities(&[when_enters(&add_counters(
        Objects::Other(&Objects::All {
            zone: Zone::Battlefield,
            filter: CREATURES_YOU_CONTROL,
        }),
        CounterKind::PlusOne,
        1,
    ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const RUNNERS: &str = Card::SupplyRunners;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("each other creature you control gets a +1/+1 counter")
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Prop::Flier,
                    Prop::Vanilla,
                ],
            )
            .hand(P0, &[RUNNERS])
            .actions(&[
                ME.casts(RUNNERS),
                Resolve,
                stack(1),
                Resolve,
                ME.counters(Prop::Flier, CounterKind::PlusOne, 1),
                ME.counters(Prop::Vanilla, CounterKind::PlusOne, 1),
                ME.pt(Prop::Flier, 4, 2),
                // The Dog is no "other" creature.
                ME.counters(RUNNERS, CounterKind::PlusOne, 0),
                ME.pt(RUNNERS, 2, 2),
            ]),
        // "You control": the opponent's creature is left alone.
        Scenario::new("a creature an opponent controls gets no counter")
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .battlefield(P1, &[Prop::Flier])
            .hand(P0, &[RUNNERS])
            .actions(&[
                ME.casts(RUNNERS),
                Resolve,
                stack(1),
                Resolve,
                THEM.counters(Prop::Flier, CounterKind::PlusOne, 0),
                ME.counters(RUNNERS, CounterKind::PlusOne, 0),
            ]),
    ];
}
