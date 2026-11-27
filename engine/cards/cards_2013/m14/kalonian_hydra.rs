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

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("attacking doubles +1/+1 counters on creatures you control")
                .permanents(
                    P0,
                    &[
                        Permanent::new(Card::KalonianHydra).counters(CounterKind::PlusOne, 4),
                        Permanent::new(Prop::Vanilla).counters(CounterKind::PlusOne, 1),
                    ],
                )
                .actions(&[
                    ME.attacks_with(Card::KalonianHydra),
                    ME.done_attacking(),
                    Resolve,
                    advance_to(1, TurnStep::EndOfCombat),
                    ME.counters(Card::KalonianHydra, CounterKind::PlusOne, 8),
                    ME.counters(Prop::Vanilla, CounterKind::PlusOne, 2),
                    THEM.life(12),
                ]),
        ];
}
