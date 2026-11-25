// Generated from the card store by `just card header`: notes go below the end marker.
//! Wizard Class {U}
//! Enchantment — Class
//!
//! (Gain the next level as a sorcery to add its ability.)
//! You have no maximum hand size.
//! {2}{U}: Level 2
//! When this Class becomes level 2, draw two cards.
//! {4}{U}: Level 3
//! Whenever you draw a card, put a +1/+1 counter on target creature you control.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::WizardClass
    .abilities(&[
        Ability::rule_change(&RuleChange {
            players: Players::You,
            what: Governed::Player(PlayerRules::NO_MAXIMUM_HAND_SIZE),
            when: When::AsUsual,
        }),
        Ability::level(2, Cost::mana(mana("{2}{U}"))),
        Ability::becomes_class_level::<2>().effect(&draw(you(), 2)),
        Ability::level(3, Cost::mana(mana("{4}{U}"))),
        Ability::triggered(&Trigger::draws(Players::You))
            .at_class_level(3)
            .target(&TargetSpec::permanent(creature_you_control()))
            .effect(&add_counters(target(), CounterKind::PlusOne, 1)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const CLASS: &str = Card::WizardClass;
    const COPY: &str = Card::CleverImpersonator;
    const DRAW: &str = Card::CerebralVortex;
    const CREATURE: &str = Prop::Vanilla;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Wizard Class gains each ability in order")
            .battlefield(
                P0,
                &[
                    CLASS,
                    CREATURE,
                    Prop::Flier,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P0, &[Card::Forest])
            .library(P0, &[Card::Forest; 6])
            .library(P1, &[Card::Mountain; 3])
            .actions(&[
                ME.deciding(DecisionKind::Priority, 3),
                ME.activates(CLASS, 1),
                stack(1),
                ME.tapped(Card::Island, 3),
                Resolve,
                stack(1),
                Resolve,
                ME.has_cards(Zone::Hand, 3),
                ME.counters(CREATURE, CounterKind::PlusOne, 0),
                // With enough mana for level two again, only playing a land is offered.
                ME.deciding(DecisionKind::Priority, 2),
                ME.passes(),
                ME.done_attacking(),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.passes(),
                advance_to(3, TurnStep::Upkeep),
                // Level three is affordable, but neither level bar can be used in upkeep.
                ME.deciding(DecisionKind::Priority, 1),
                advance_to(3, TurnStep::PrecombatMain),
                ME.activates(CLASS, 3),
                ME.tapped(Card::Island, 5),
                Resolve,
                ME.counters(CREATURE, CounterKind::PlusOne, 0),
                ME.passes(),
                ME.done_attacking(),
                advance_to(4, TurnStep::PrecombatMain),
                THEM.passes(),
                advance_to(5, TurnStep::Upkeep),
                ME.passes(),
                ME.targets(CREATURE),
                Resolve,
                ME.counters(CREATURE, CounterKind::PlusOne, 1),
            ]),
        Scenario::new("Wizard Class cannot gain level two in upkeep")
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, &[CLASS, Card::Island, Card::Island, Card::Island])
            .actions(&[
                advance_to(3, TurnStep::Upkeep),
                ME.deciding(DecisionKind::Priority, 1),
            ]),
        Scenario::new("A copy of Wizard Class can start its own levels")
            .interaction(&[COPY])
            .battlefield(P0, &[CLASS, Card::Island, Card::Island, Card::Island])
            .battlefield(P1, &[Card::Island; 7])
            .hand(P1, &[COPY])
            .library(P0, &[Card::Mountain; 5])
            .library(P1, &[Card::Forest; 4])
            .actions(&[
                ME.activates(CLASS, 1),
                Resolve,
                Resolve,
                advance_to(2, TurnStep::PrecombatMain),
                THEM.casts(COPY),
                Resolve,
                THEM.picks_up_to(CLASS),
                THEM.controls(CLASS, 1),
                THEM.activates(CLASS, 1),
                Resolve,
                Resolve,
                THEM.has_cards(Zone::Hand, 3),
                ME.has_cards(Zone::Hand, 2),
            ]),
        Scenario::new("Wizard Class removes the cleanup hand limit")
            .battlefield(P0, &[CLASS])
            .hand(P0, &[Card::Island; 8])
            .hand(P1, &[Card::Mountain; 8])
            .library(P0, &[Card::Forest; 3])
            .library(P1, &[Card::Mountain; 3])
            .actions(&[
                advance_to(2, TurnStep::PrecombatMain),
                ME.has_cards(Zone::Hand, 8),
                advance_to(3, TurnStep::Upkeep),
                THEM.has_cards(Zone::Hand, 7),
            ]),
        Scenario::new("Level three triggers for each card drawn")
            .interaction(&[DRAW])
            .battlefield(
                P0,
                &[
                    CLASS,
                    CREATURE,
                    Prop::Flier,
                    Card::Mountain,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .battlefield(P1, &[Prop::Vanilla])
            .hand(P0, &[DRAW])
            .library(P0, &[Card::Forest; 6])
            .actions(&[
                ME.activates(CLASS, 1),
                Resolve,
                Resolve,
                ME.activates(CLASS, 3),
                Resolve,
                ME.casts(DRAW),
                ME.targets_player(ME),
                Resolve,
                ME.deciding(DecisionKind::Target, 2),
                ME.targets(CREATURE),
                ME.targets(CREATURE),
                Resolve,
                Resolve,
                ME.counters(CREATURE, CounterKind::PlusOne, 2),
            ]),
    ];
}
