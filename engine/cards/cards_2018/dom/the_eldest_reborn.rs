// Generated from the card store by `just card header`: notes go below the end marker.
//! The Eldest Reborn {4}{B}
//! Enchantment — Saga
//!
//! (As this Saga enters and after your draw step, add a lore counter. Sacrifice after III.)
//! I — Each opponent sacrifices a creature or planeswalker of their choice.
//! II — Each opponent discards a card.
//! III — Put target creature or planeswalker card from a graveyard onto the battlefield under your
//! control.
// End of the generated card header.

use crate::prelude::*;

const CREATURE_OR_PLANESWALKER: Filter = Filter::Types(Types::CREATURE.union(Types::PLANESWALKER));

pub static CARD: CardDef = Printed::TheEldestReborn
    .abilities(&[
        Ability::chapter(1).effect(&sequence(&[
            for_each_player(
                each_opponent(),
                &choose(
                    that_player(),
                    permanents_controlled_by(&that_player(), CREATURE_OR_PLANESWALKER),
                    Prompt::new(Purpose::Choose).most(1),
                ),
            ),
            sacrifice(those()),
        ])),
        Ability::chapter(2).effect(&discard(each_opponent(), 1)),
        Ability::chapter(3)
            .target(&TargetSpec::graveyard_card(CREATURE_OR_PLANESWALKER))
            .effect(&put_onto_battlefield(&onto_battlefield_under(
                target(),
                you(),
            ))),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SAGA: &str = Card::TheEldestReborn;
    const WALKER: &str = Prop::Planeswalker;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Three chapters resolve before the Saga is sacrificed")
            .battlefield(P0, &[Card::Swamp; 5])
            .hand(P0, &[SAGA])
            .library(P0, &[Card::Swamp; 3])
            .battlefield(P1, &[WALKER, Prop::Vanilla, Card::Island])
            .hand(P1, &[Card::Plains, Card::Mountain, Card::Swamp])
            .library(P1, &[Card::Forest, Card::Island, Card::Forest])
            .graveyard(P1, &[Prop::Flier])
            .actions(&[
                ME.casts(SAGA),
                Resolve,
                ME.counters(SAGA, CounterKind::Lore, 1),
                stack(1),
                Resolve,
                THEM.picks(WALKER),
                THEM.has(Zone::Graveyard, WALKER, 1),
                THEM.controls(Prop::Vanilla, 1),
                advance_to(2, TurnStep::PrecombatMain),
                ME.counters(SAGA, CounterKind::Lore, 1),
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.done_attacking(),
                advance_to(3, TurnStep::PrecombatMain),
                ME.counters(SAGA, CounterKind::Lore, 2),
                stack(1),
                Resolve,
                THEM.picks(Card::Mountain),
                THEM.has(Zone::Graveyard, Card::Mountain, 1),
                THEM.has_cards(Zone::Hand, 3),
                ME.has_cards(Zone::Hand, 1),
                advance_to(4, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.done_attacking(),
                advance_to(5, TurnStep::Draw),
                ME.counters(SAGA, CounterKind::Lore, 2),
                ME.passes(),
                ME.targets(WALKER),
                ME.counters(SAGA, CounterKind::Lore, 3),
                stack(1),
                Resolve,
                ME.controls(WALKER, 1),
                ME.tapped(WALKER, 0),
                ME.has(Zone::Graveyard, SAGA, 1),
            ]),
        Scenario::new("The third chapter resolves after the Saga leaves")
            .interaction(&[Card::GetLost, Card::LightningBolt])
            .battlefield(P0, &[Card::Mountain])
            .hand(P0, &[Card::LightningBolt])
            .permanents(P0, &[Permanent::new(SAGA).counters(CounterKind::Lore, 2)])
            .graveyard(P1, &[Prop::Vanilla, Prop::Flier])
            .battlefield(P1, &[Card::Plains, Card::Plains])
            .hand(P1, &[Card::GetLost])
            .at(3, P0, TurnStep::Draw)
            .actions(&[
                advance_to(3, TurnStep::Draw),
                ME.passes(),
                THEM.passes(),
                ME.targets(Prop::Flier),
                ME.controls(SAGA, 1),
                ME.passes(),
                THEM.casts(Card::GetLost),
                Resolve,
                ME.has(Zone::Graveyard, SAGA, 1),
                stack(1),
                Resolve,
                ME.controls(Prop::Flier, 1),
            ]),
        Scenario::new("The Saga is sacrificed when the third chapter has no legal target")
            .interaction(&[Card::LightningBolt])
            .battlefield(P0, &[Card::Mountain])
            .hand(P0, &[Card::LightningBolt])
            .permanents(P0, &[Permanent::new(SAGA).counters(CounterKind::Lore, 2)])
            .graveyard(P1, &[Card::Island])
            .at(3, P0, TurnStep::Draw)
            .actions(&[
                advance_to(3, TurnStep::PrecombatMain),
                stack(0),
                ME.has(Zone::Graveyard, SAGA, 1),
            ]),
        Scenario::new("An animated Saga with no abilities enters without lore")
            .interaction(&[Card::Opalescence, Card::Humility])
            .battlefield(
                P0,
                &[
                    Card::Swamp,
                    Card::Swamp,
                    Card::Swamp,
                    Card::Swamp,
                    Card::Swamp,
                    Card::Opalescence,
                    Card::Humility,
                ],
            )
            .hand(P0, &[SAGA])
            .actions(&[
                ME.casts(SAGA),
                Resolve,
                ME.controls(SAGA, 1),
                ME.counters(SAGA, CounterKind::Lore, 0),
                stack(0),
            ]),
        Scenario::new("An animated Saga with no abilities keeps its lore and stays")
            .interaction(&[Card::Opalescence, Card::DressDown])
            .battlefield(P0, &[Card::Opalescence, Card::DressDown])
            .permanents(P0, &[Permanent::new(SAGA).counters(CounterKind::Lore, 3)])
            .at(3, P0, TurnStep::Draw)
            .actions(&[
                advance_to(3, TurnStep::PrecombatMain),
                stack(0),
                ME.controls(SAGA, 1),
                ME.counters(SAGA, CounterKind::Lore, 3),
            ]),
    ];
}
