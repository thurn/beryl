// Generated from the card store by `just card header`: notes go below the end marker.
//! Chronicler of Heroes {1}{G}{W}
//! Creature — Centaur Wizard, 3/3
//!
//! When this creature enters, draw a card if you control a creature with a +1/+1 counter on it.
// End of the generated card header.

use crate::prelude::*;

const YOUR_CREATURES: Objects = Objects::All {
    zone: Zone::Battlefield,
    filter: Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
};

pub static CARD: CardDef = Printed::ChroniclerOfHeroes
    .abilities(&[when_enters(&if_then(
        Condition::Compare(
            Amount::Counters(YOUR_CREATURES, CounterKind::PlusOne),
            Comparison::AtLeast,
            Amount::Fixed(1),
        ),
        &draw(you(), 1),
    ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const CHRONICLER: &str = Card::ChroniclerOfHeroes;
    const EMPEROR: &str = Card::TheWanderingEmperor;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("a counter added in response lets the Chronicler draw")
                .interaction(&[EMPEROR])
                .hand(P0, &[CHRONICLER, EMPEROR])
                .battlefield(
                    P0,
                    &[
                        Card::Forest,
                        Card::Plains,
                        Card::Plains,
                        Card::Plains,
                        Card::Plains,
                        Card::Plains,
                        Card::Plains,
                        Prop::Vanilla,
                    ],
                )
                .library(P0, &[Card::Island, Card::Forest])
                .library(P1, &[Card::Mountain, Card::Forest])
                .actions(&[
                    ME.casts(CHRONICLER),
                    Resolve,
                    stack(1),
                    ME.casts(EMPEROR),
                    Resolve,
                    ME.activates(EMPEROR, 1),
                    ME.target_count(1),
                    ME.targets(CHRONICLER),
                    Resolve,
                    ME.counters(CHRONICLER, CounterKind::PlusOne, 1),
                    Resolve,
                    ME.has(Zone::Hand, Card::Island, 1),
                    ME.has(Zone::Hand, Card::Forest, 0),
                    THEM.has(Zone::Hand, Card::Mountain, 0),
                    ME.plays(Card::Island),
                    stack(0),
                    ME.has(Zone::Hand, Card::Forest, 0),
                ]),
        ];
}
