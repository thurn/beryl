//! Oust, Get Lost and the Map token's explore against stand-in creatures and planeswalkers,
//! through the scenario runner, until the cube's own creatures and planeswalkers join the
//! card table. These scenarios are not in [`crate::SCENARIOS`]: the oracles know no stand-ins.

use beryl_engine::card::{CardDef, Face, ManaCost, Types};
use beryl_engine::state::CounterKind;
use beryl_engine::{CardTable, DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario, run};

use crate::blue::island;
use crate::names;
use crate::white::{get_lost, oust, plains};

const BEAR: &str = "Stand-in Bear";
const WALKER: &str = "Stand-in Walker";
const MAP: &str = get_lost::MAP.name;

static BEAR_CARD: CardDef = CardDef {
    name: BEAR,
    faces: &[Face {
        cost: ManaCost::new(1, [1, 0, 0, 0, 0, 0]),
        types: Types::CREATURE,
        power: Some(2),
        toughness: Some(2),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static WALKER_CARD: CardDef = CardDef {
    name: WALKER,
    faces: &[Face {
        cost: ManaCost::new(2, [1, 0, 0, 0, 0, 0]),
        types: Types::PLANESWALKER,
        loyalty: Some(3),
        ..Face::EMPTY
    }],
    tokens: &[],
};

static TABLE: CardTable = CardTable::new(&[
    &BEAR_CARD,
    &WALKER_CARD,
    &oust::CARD,
    &get_lost::CARD,
    &plains::CARD,
    &island::CARD,
]);

fn check(scenario: &Scenario) {
    if let Err(e) = run(&TABLE, scenario) {
        panic!("{e}");
    }
}

const fn count(player: u8, zone: Zone, card: &'static str, count: u8) -> Action {
    Action::assert(Assertion::Count {
        player,
        zone,
        card: Some(card),
        count,
    })
}

const fn library(player: u8, top: &'static [&'static str]) -> Action {
    Action::assert(Assertion::Library { player, top })
}

/// P0 casts Oust on P1's Bear. The Plains left in P0's hand keeps the game in P0's main
/// phase once Oust has resolved.
const fn oust_onto(top: &'static [&'static str]) -> Scenario {
    const PLAINS: &[Permanent] = &[Permanent::new(names::PLAINS)];
    const BEARS: &[Permanent] = &[Permanent::new(BEAR)];
    Scenario::new("Oust")
        .battlefield(P0, PLAINS)
        .hand(P0, &[names::OUST, names::PLAINS])
        .battlefield(P1, BEARS)
        .library(P1, top)
}

#[test]
fn oust_on_an_empty_library_makes_the_creature_its_only_card() {
    const S: Scenario = oust_onto(&[]).actions(&[
        Action::cast(P0, names::OUST),
        library(P1, &[BEAR]),
        Action::assert(Assertion::Life {
            player: P1,
            life: 23,
        }),
        Action::assert(Assertion::Life {
            player: P0,
            life: 20,
        }),
    ]);
    check(&S);
}

#[test]
fn oust_on_a_library_of_one_puts_the_creature_under_it() {
    const S: Scenario = oust_onto(&[names::ISLAND]).actions(&[
        Action::cast(P0, names::OUST),
        library(P1, &[names::ISLAND, BEAR]),
    ]);
    check(&S);
}

#[test]
fn oust_puts_the_creature_second_from_the_top() {
    const S: Scenario = oust_onto(&[names::ISLAND, names::PLAINS]).actions(&[
        Action::cast(P0, names::OUST),
        library(P1, &[names::ISLAND, BEAR, names::PLAINS]),
        count(P1, Zone::Battlefield, BEAR, 0),
        count(P0, Zone::Graveyard, names::OUST, 1),
    ]);
    check(&S);
}

const THREE_PLAINS: &[Permanent] = &[
    Permanent::new(names::PLAINS),
    Permanent::new(names::PLAINS),
    Permanent::new(names::PLAINS),
];

/// P1 casts the planeswalker, so it enters with its loyalty; then P0 destroys it at instant
/// speed.
#[test]
fn get_lost_on_a_planeswalker_gives_its_controller_two_maps() {
    const S: Scenario = Scenario::new("Get Lost on a planeswalker")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, THREE_PLAINS)
        .hand(P0, &[names::GET_LOST])
        .library(P0, &[names::PLAINS])
        .battlefield(P1, THREE_PLAINS)
        .hand(P1, &[WALKER, names::ISLAND])
        .actions(&[
            Action::cast(P1, WALKER),
            count(P1, Zone::Battlefield, WALKER, 1),
            Action::pass(P1),
            Action::cast(P0, names::GET_LOST),
            count(P1, Zone::Graveyard, WALKER, 1),
            count(P1, Zone::Battlefield, MAP, 2),
            count(P0, Zone::Battlefield, MAP, 0),
        ]);
    check(&S);
}

/// In P1's turn 2, P0 destroys one of P1's two Bears with Get Lost at instant speed; P1 then
/// sacrifices a Map, paying {1} with their Plains, and their other Bear explores the top of
/// `top`. The Island in P1's hand keeps their priority a decision.
const fn explore(top: &'static [&'static str]) -> Scenario {
    const BEARS_AND_PLAINS: &[Permanent] = &[
        Permanent::new(BEAR),
        Permanent::new(BEAR),
        Permanent::new(names::PLAINS),
    ];
    Scenario::new("a Map explores")
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, THREE_PLAINS)
        .hand(P0, &[names::GET_LOST])
        .library(P0, &[names::PLAINS])
        .battlefield(P1, BEARS_AND_PLAINS)
        .hand(P1, &[names::ISLAND])
        .library(P1, top)
}

const LET_P0_ACT: Action = Action::pass(P1);
const GET_LOST: Action = Action::cast(P0, names::GET_LOST);
const SACRIFICE_A_MAP: Action = Action::activate(P1, MAP, 0);

const fn counters(count: u8) -> Action {
    Action::assert(Assertion::Counters {
        player: P1,
        card: BEAR,
        kind: CounterKind::PlusOne,
        count,
    })
}

#[test]
fn a_map_explores_a_land_into_its_controllers_hand() {
    const S: Scenario = explore(&[names::PLAINS, names::ISLAND]).actions(&[
        LET_P0_ACT,
        GET_LOST,
        count(P1, Zone::Graveyard, BEAR, 1),
        count(P1, Zone::Battlefield, MAP, 2),
        SACRIFICE_A_MAP,
        count(P1, Zone::Battlefield, MAP, 1),
        count(P1, Zone::Hand, names::PLAINS, 1),
        library(P1, &[names::ISLAND]),
        counters(0),
    ]);
    check(&S);
}

#[test]
fn a_map_explores_a_nonland_into_a_counter_and_the_graveyard() {
    const S: Scenario = explore(&[names::OUST, names::ISLAND]).actions(&[
        LET_P0_ACT,
        GET_LOST,
        SACRIFICE_A_MAP,
        Action::answer(P1, DecisionKind::YesNo, Pick::Yes),
        counters(1),
        count(P1, Zone::Graveyard, names::OUST, 1),
        library(P1, &[names::ISLAND]),
    ]);
    check(&S);
}

#[test]
fn a_map_explores_a_nonland_left_on_top() {
    const S: Scenario = explore(&[names::OUST, names::ISLAND]).actions(&[
        LET_P0_ACT,
        GET_LOST,
        SACRIFICE_A_MAP,
        Action::answer(P1, DecisionKind::YesNo, Pick::No),
        counters(1),
        count(P1, Zone::Graveyard, names::OUST, 0),
        library(P1, &[names::OUST, names::ISLAND]),
    ]);
    check(&S);
}

#[test]
fn a_map_explores_an_empty_library_into_a_counter() {
    const S: Scenario = explore(&[]).actions(&[LET_P0_ACT, GET_LOST, SACRIFICE_A_MAP, counters(1)]);
    check(&S);
}
