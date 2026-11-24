//! Triggered abilities ([CR 603](https://yawgatog.com/resources/magic-rules/#R603)).

use beryl_engine::card::{Castings, FaceIx};
use beryl_engine::state::CounterKind;
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, P0, P1, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to, stack};

use crate::{names, props};

const SKYDIVER: &str = names::THIEVING_SKYDIVER;
const SMALL_ARTIFACT: &str = props::EQUIPMENT.card;
const LARGE_ARTIFACT: &str = names::TRINISPHERE;
const KICK_SKYDIVER: Action = Action::Cast {
    player: P0,
    card: SKYDIVER,
    face: FaceIx::FRONT,
    costs: Castings::only(0),
};

/// The enters ability uses the X of its spell. At X=1 the Scimitar is the only
/// target, and at X=3 the controller may choose the Trinisphere instead.
pub const X_FROM_SPELL: &[Scenario] = &[
    Scenario::new("An enters trigger with X excludes the larger artifact at X=1")
        .interaction(&[LARGE_ARTIFACT])
        .battlefield(P0, &[names::ISLAND; 5])
        .battlefield(P1, &[SMALL_ARTIFACT, LARGE_ARTIFACT])
        .hand(P0, &[SKYDIVER])
        .actions(&[
            KICK_SKYDIVER,
            ME.x(1),
            Resolve,
            Resolve,
            ME.controls(SMALL_ARTIFACT, 1),
            THEM.controls(LARGE_ARTIFACT, 1),
        ]),
    Scenario::new("An enters trigger with X can target the larger artifact at X=3")
        .interaction(&[LARGE_ARTIFACT])
        .battlefield(P0, &[names::ISLAND; 5])
        .battlefield(P1, &[SMALL_ARTIFACT, LARGE_ARTIFACT])
        .hand(P0, &[SKYDIVER])
        .actions(&[
            KICK_SKYDIVER,
            ME.x(3),
            Resolve,
            ME.deciding(DecisionKind::Target, 2),
            ME.targets(LARGE_ARTIFACT),
            Resolve,
            ME.controls(LARGE_ARTIFACT, 1),
            THEM.controls(SMALL_ARTIFACT, 1),
        ]),
];

/// The step trigger can fire during an opponent's turn as well as its controller's.
pub const OPPONENTS_STEP: &[Scenario] =
    &[
        Scenario::new("Dress Down is sacrificed at the start of the opposing end step")
            .at(2, P1, TurnStep::PostcombatMain)
            .battlefield(P0, &[names::DRESS_DOWN])
            .actions(&[
                advance_to(2, TurnStep::End),
                stack(1),
                Resolve,
                ME.controls(names::DRESS_DOWN, 0),
                ME.has(Zone::Graveyard, names::DRESS_DOWN, 1),
            ]),
    ];

const OVERLORD: &str = names::OVERLORD_OF_THE_MISTMOORS;

/// “Your end step” names the ability controller's turn. The opposing end step
/// passes without removing the counter.
pub const CONTROLLERS_END_STEP: &[Scenario] =
    &[Scenario::new("Your end step excludes the opposing turn")
        .at(1, P0, TurnStep::PostcombatMain)
        .battlefield(P0, &[names::PLAINS; 4])
        .hand(P0, &[OVERLORD])
        .actions(&[
            Action::Cast {
                player: P0,
                card: OVERLORD,
                face: FaceIx::FRONT,
                costs: Castings::only(0),
            },
            Resolve,
            Resolve,
            advance_to(1, TurnStep::End),
            stack(1),
            Resolve,
            ME.counters(OVERLORD, CounterKind::Time, 3),
            advance_to(2, TurnStep::End),
            stack(0),
            ME.counters(OVERLORD, CounterKind::Time, 3),
            advance_to(3, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.done_attacking(),
            advance_to(3, TurnStep::End),
            stack(1),
            Resolve,
            ME.counters(OVERLORD, CounterKind::Time, 2),
        ])];
