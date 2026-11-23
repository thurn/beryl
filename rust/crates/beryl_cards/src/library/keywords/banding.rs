//! Banding changes who assigns combat damage, including for a creature fighting a bander
//! that isn't in an attacking band.

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, P0, P1, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to};

use crate::{names, props};

const HERO: &str = names::BENALISH_HERO;
const INFANTRY: &str = names::BENALISH_INFANTRY;
const GROUND: &str = props::VANILLA.card;
const FLIER: &str = props::FLIER.card;
const TRAMPLER: &str = names::COLOSSAL_DREADMAW;
const REMOVAL: &str = names::GET_LOST;

const START: Action = advance_to(1, TurnStep::BeginningOfCombat);
const END: Action = advance_to(1, TurnStep::EndOfCombat);

const fn band(card: &'static str, first: Option<&'static str>) -> Action {
    ME.answers(DecisionKind::Attacker, Pick::Band { card, first })
}

const fn block(card: &'static str) -> Action {
    THEM.chooses(DecisionKind::Blocker, card)
}

const fn block_target(card: &'static str) -> Action {
    THEM.chooses(DecisionKind::BlockTarget, card)
}

pub const HERO_SCENARIOS: &[Scenario] = &[
    Scenario::new("two banders share the block and choose where all its damage goes")
        .battlefield(P0, &[HERO, INFANTRY])
        .battlefield(P1, &[GROUND])
        .actions(&[
            START,
            ME.passes(),
            band(HERO, None),
            band(INFANTRY, Some(HERO)),
            block(GROUND),
            block_target(HERO),
            ME.answers(DecisionKind::DamageDivision, Pick::Number { n: 0 }),
            END,
            ME.controls(HERO, 1),
            ME.controls(INFANTRY, 1),
            THEM.has(Zone::Graveyard, GROUND, 1),
            THEM.life(20),
        ]),
    Scenario::new("blocking the ground member also blocks the flying member")
        .battlefield(P0, &[HERO, FLIER])
        .battlefield(P1, &[GROUND])
        .actions(&[
            START,
            ME.passes(),
            band(HERO, None),
            band(FLIER, Some(HERO)),
            block(GROUND),
            ME.answers(DecisionKind::DamageDivision, Pick::Number { n: 2 }),
            END,
            ME.has(Zone::Graveyard, HERO, 1),
            ME.controls(FLIER, 1),
            THEM.has(Zone::Graveyard, GROUND, 1),
            THEM.life(20),
        ]),
    Scenario::new("a band accepts no second creature without banding")
        .battlefield(P0, &[HERO, GROUND, FLIER])
        .actions(&[
            START,
            ME.passes(),
            band(HERO, None),
            ME.deciding(DecisionKind::Attacker, 5),
            band(GROUND, Some(HERO)),
            ME.deciding(DecisionKind::Attacker, 2),
            ME.attacks_with(FLIER),
            END,
            THEM.life(14),
        ]),
    Scenario::new("a band follows the first creature to the same planeswalker")
        .battlefield(P0, &[HERO, GROUND])
        .battlefield(P1, &[props::PLANESWALKER.card])
        .actions(&[
            START,
            ME.passes(),
            band(HERO, None),
            ME.chooses(DecisionKind::AttackTarget, props::PLANESWALKER.card),
            band(GROUND, Some(HERO)),
            END,
            THEM.life(20),
            THEM.controls(props::PLANESWALKER.card, 0),
        ]),
    Scenario::new("the hero can attack alone")
        .battlefield(P0, &[HERO])
        .actions(&[
            START,
            ME.passes(),
            ME.attacks_with(HERO),
            END,
            THEM.life(19),
        ]),
];

pub const INFANTRY_SCENARIOS: &[Scenario] = &[
    Scenario::new(
        "the defender assigns a trampler's damage to its banding blocker and other blocker",
    )
    .battlefield(P0, &[TRAMPLER])
    .battlefield(P1, &[GROUND, INFANTRY])
    .actions(&[
        START,
        ME.passes(),
        ME.attacks_with(TRAMPLER),
        block(GROUND),
        block(INFANTRY),
        THEM.answers(DecisionKind::DamageDivision, Pick::Number { n: 6 }),
        END,
        THEM.has(Zone::Graveyard, GROUND, 1),
        THEM.controls(INFANTRY, 1),
        THEM.life(20),
    ]),
    Scenario::new("after a band member leaves combat the blocker damages only the survivor")
        .interaction(&[HERO, REMOVAL])
        .battlefield(P0, &[INFANTRY, HERO])
        .battlefield(P1, &[GROUND, names::PLAINS, names::PLAINS])
        .hand(P1, &[REMOVAL])
        .actions(&[
            START,
            ME.passes(),
            THEM.passes(),
            band(HERO, None),
            band(INFANTRY, Some(HERO)),
            THEM.passes(),
            block(GROUND),
            block_target(HERO),
            THEM.casts(REMOVAL),
            THEM.targets(HERO),
            END,
            ME.controls(INFANTRY, 1),
            ME.has(Zone::Graveyard, HERO, 1),
            THEM.has(Zone::Graveyard, GROUND, 1),
            THEM.life(20),
        ]),
    Scenario::new("trample counts the damage assigned by the other member of a band")
        .battlefield(P0, &[INFANTRY, TRAMPLER])
        .battlefield(P1, &[GROUND])
        .actions(&[
            START,
            ME.passes(),
            band(INFANTRY, None),
            band(TRAMPLER, Some(INFANTRY)),
            block(GROUND),
            block_target(INFANTRY),
            ME.answers(DecisionKind::DamageDivision, Pick::Number { n: 0 }),
            ME.answers(DecisionKind::DamageDivision, Pick::Number { n: 2 }),
            END,
            ME.controls(INFANTRY, 1),
            ME.controls(TRAMPLER, 1),
            THEM.has(Zone::Graveyard, GROUND, 1),
            THEM.life(14),
        ]),
    Scenario::new("two tramplers can share a lethal assignment to a blocker of the band")
        .interaction(&[names::DUSKDALE_WURM, names::PALACE_GUARD])
        .battlefield(P0, &[INFANTRY, TRAMPLER, names::DUSKDALE_WURM])
        .battlefield(P1, &[names::PALACE_GUARD])
        .actions(&[
            START,
            ME.passes(),
            band(INFANTRY, None),
            band(TRAMPLER, Some(INFANTRY)),
            ME.attacks_with(names::DUSKDALE_WURM),
            block(names::PALACE_GUARD),
            block_target(INFANTRY),
            block(names::PALACE_GUARD),
            ME.answers(DecisionKind::DamageDivision, Pick::Number { n: 0 }),
            ME.deciding(DecisionKind::DamageDivision, 5),
            ME.answers(DecisionKind::DamageDivision, Pick::Number { n: 3 }),
            ME.answers(DecisionKind::DamageDivision, Pick::Number { n: 0 }),
            ME.answers(DecisionKind::DamageDivision, Pick::Number { n: 0 }),
            END,
            THEM.has(Zone::Graveyard, names::PALACE_GUARD, 1),
            THEM.life(10),
        ]),
    Scenario::new("losing banding keeps the band but returns damage assignment to the blocker")
        .interaction(&[HERO, names::DRESS_DOWN])
        .battlefield(P0, &[HERO, INFANTRY])
        .battlefield(P1, &[GROUND, names::ISLAND, names::ISLAND])
        .hand(P1, &[names::DRESS_DOWN])
        .actions(&[
            START,
            ME.passes(),
            THEM.passes(),
            band(HERO, None),
            band(INFANTRY, Some(HERO)),
            THEM.casts(names::DRESS_DOWN),
            Resolve,
            ME.passes(),
            block(GROUND),
            block_target(HERO),
            THEM.answers(DecisionKind::DamageDivision, Pick::Number { n: 1 }),
            END,
            ME.has(Zone::Graveyard, HERO, 1),
            ME.controls(INFANTRY, 1),
            THEM.has(Zone::Graveyard, GROUND, 1),
            THEM.life(20),
        ]),
    Scenario::new("the infantry can block alone")
        .battlefield(P0, &[GROUND])
        .battlefield(P1, &[INFANTRY])
        .actions(&[
            START,
            ME.passes(),
            ME.attacks_with(GROUND),
            block(INFANTRY),
            END,
            THEM.controls(INFANTRY, 1),
            ME.has(Zone::Graveyard, GROUND, 1),
            THEM.life(20),
        ]),
];
