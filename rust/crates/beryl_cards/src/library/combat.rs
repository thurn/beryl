//! Combat declarations and damage ([CR 506] through [CR 511]).
//!
//! [CR 506]: https://yawgatog.com/resources/magic-rules/#R506
//! [CR 511]: https://yawgatog.com/resources/magic-rules/#R511

use beryl_engine::card::{Colors, Keywords, Subtype, Subtypes, Types};
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};
use beryl_scenario_types::{ME, PermanentState, Resolve, SnapshotRef, THEM, advance_to};

use crate::{names, props};

const GROUND: &str = props::VANILLA.card;
const FLIER: &str = props::FLIER.card;
const KNIGHT: &str = names::WHITE_KNIGHT;
const DREADMAW: &str = names::COLOSSAL_DREADMAW;

pub const DECLARATIONS: &[Scenario] = &[
    Scenario::new("two interchangeable attackers are offered one copy at a time")
        .battlefield(P0, &[GROUND, GROUND, FLIER])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.deciding(DecisionKind::Attacker, 3),
            ME.attacks_with(GROUND),
            ME.deciding(DecisionKind::Attacker, 3),
            ME.attacks_with(GROUND),
            ME.done_attacking(),
            advance_to(1, TurnStep::DeclareAttackers),
            ME.attacking(GROUND, 2),
            ME.tapped(GROUND, 2),
            ME.tapped(FLIER, 0),
            advance_to(1, TurnStep::PostcombatMain),
            ME.attacking(GROUND, 0),
            THEM.life(16),
            ME.life(20),
        ]),
    Scenario::new("a tapped creature and a creature that just entered cannot attack")
        .permanents(
            P0,
            &[
                Permanent::new(GROUND).sick(),
                Permanent::new(FLIER).tapped(),
            ],
        )
        .battlefield(P1, &[GROUND])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            advance_to(1, TurnStep::PostcombatMain),
            ME.tapped(GROUND, 0),
            THEM.life(20),
        ]),
];

pub const FIRST_STRIKE: &[Scenario] = &[
    Scenario::new("a first striker kills its blocker before the blocker deals damage")
        .battlefield(P0, &[KNIGHT])
        .battlefield(P1, &[GROUND])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(KNIGHT),
            THEM.chooses(DecisionKind::Blocker, GROUND),
            advance_to(1, TurnStep::FirstStrikeDamage),
            THEM.has(Zone::Graveyard, GROUND, 1),
            ME.controls(KNIGHT, 1),
            advance_to(1, TurnStep::CombatDamage),
            ME.controls(KNIGHT, 1),
        ]),
    Scenario::new("a first striker deals damage once and the other creatures deal it second")
        .battlefield(P0, &[KNIGHT, GROUND])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(KNIGHT),
            ME.attacks_with(GROUND),
            advance_to(1, TurnStep::FirstStrikeDamage),
            THEM.life(18),
            ME.controls(GROUND, 1),
            advance_to(1, TurnStep::CombatDamage),
            THEM.life(16),
            ME.controls(GROUND, 1),
        ]),
    Scenario::new("a blocker without first strike deals damage only in the second step")
        .battlefield(P0, &[KNIGHT])
        .battlefield(P1, &[DREADMAW])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(KNIGHT),
            THEM.chooses(DecisionKind::Blocker, DREADMAW),
            advance_to(1, TurnStep::FirstStrikeDamage),
            ME.controls(KNIGHT, 1),
            STRUCK_TRAMPLER,
            advance_to(1, TurnStep::CombatDamage),
            ME.has(Zone::Graveyard, KNIGHT, 1),
            STRUCK_TRAMPLER,
        ]),
];

const STRUCK_TRAMPLER: Action = Action::assert(Assertion::PermanentState {
    object: SnapshotRef {
        zone: Zone::Battlefield,
        player: P1,
        name: Some(DREADMAW),
        index: 0,
    },
    state: PermanentState {
        colors: Colors::GREEN,
        types: Types::CREATURE,
        subtypes: Subtypes::of(&[Subtype::Dinosaur]),
        all_creature_types: false,
        keywords: Keywords::TRAMPLE,
        damage: 2,
        attached_to: None,
        pt: Some((6, 6)),
    },
});

const BLOCKER_REMOVAL: Scenario = Scenario::setup()
    .battlefield(P0, &[names::PLAINS, names::PLAINS])
    .battlefield(P1, &[GROUND])
    .hand(P0, &[names::GET_LOST]);

pub const REMOVED_BLOCKER: &[Scenario] = &[
    BLOCKER_REMOVAL
        .titled("an attacker without trample stays blocked when its blocker leaves")
        .battlefield(P0, &[names::PLAINS, names::PLAINS, FLIER])
        .battlefield(P1, &[names::SERRA_ANGEL])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(FLIER),
            ME.passes(),
            THEM.chooses(DecisionKind::Blocker, names::SERRA_ANGEL),
            advance_to(1, TurnStep::DeclareBlockers),
            ME.casts(names::GET_LOST),
            ME.targets(names::SERRA_ANGEL),
            Resolve,
            advance_to(1, TurnStep::EndOfCombat),
            ME.controls(FLIER, 1),
            THEM.has(Zone::Graveyard, names::SERRA_ANGEL, 1),
            THEM.life(20),
        ]),
    BLOCKER_REMOVAL
        .titled("a blocked attacker with trample deals all its damage after its blocker leaves")
        .battlefield(P0, &[names::PLAINS, names::PLAINS, DREADMAW])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(DREADMAW),
            ME.passes(),
            THEM.chooses(DecisionKind::Blocker, GROUND),
            advance_to(1, TurnStep::DeclareBlockers),
            ME.casts(names::GET_LOST),
            ME.targets(GROUND),
            Resolve,
            advance_to(1, TurnStep::EndOfCombat),
            THEM.has(Zone::Graveyard, GROUND, 1),
            THEM.life(14),
        ]),
];

pub const REMOVED_PLANESWALKER: &[Scenario] =
    &[
        Scenario::new("an attacker whose planeswalker left combat deals no damage")
            .battlefield(P0, &[GROUND, names::PLAINS, names::PLAINS])
            .battlefield(P1, &[props::PLANESWALKER.card])
            .hand(P0, &[names::GET_LOST])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(GROUND),
                ME.chooses(DecisionKind::AttackTarget, props::PLANESWALKER.card),
                advance_to(1, TurnStep::DeclareAttackers),
                ME.casts(names::GET_LOST),
                ME.targets(props::PLANESWALKER.card),
                Resolve,
                ME.attacking(GROUND, 1),
                advance_to(1, TurnStep::EndOfCombat),
                ME.life(20),
                THEM.life(20),
            ]),
    ];

/// The later combat steps still happen after the sole attacker leaves.
pub const REMOVED_ATTACKERS: &[Scenario] =
    &[
        Scenario::new("combat continues after the only attacker is removed")
            .battlefield(P0, &[GROUND])
            .battlefield(P1, &[names::PLAINS, names::PLAINS])
            .hand(P1, &[names::GET_LOST])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                THEM.passes(),
                ME.attacks_with(GROUND),
                advance_to(1, TurnStep::DeclareAttackers),
                ME.passes(),
                THEM.casts(names::GET_LOST),
                Resolve,
                advance_to(1, TurnStep::DeclareBlockers),
                advance_to(1, TurnStep::CombatDamage),
                ME.has(Zone::Graveyard, GROUND, 1),
                THEM.life(20),
            ]),
    ];

pub const CANNOT_BLOCK: &[Scenario] = &[
    Scenario::new("the defender is offered only creatures that can block")
        .battlefield(P0, &[GROUND])
        .battlefield(P1, &[names::BLOODGHAST, FLIER])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(GROUND),
            THEM.deciding(DecisionKind::Blocker, 2),
            THEM.chooses(DecisionKind::Blocker, FLIER),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(20),
            THEM.has(Zone::Graveyard, FLIER, 1),
        ]),
    Scenario::new("the defender is not asked to block when no creature can block")
        .battlefield(P0, &[GROUND])
        .battlefield(P1, &[names::BLOODGHAST])
        .actions(&[
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(GROUND),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(18),
        ]),
];
