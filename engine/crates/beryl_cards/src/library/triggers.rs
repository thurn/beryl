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

const HARMONIC: &str = names::HARMONIC_SLIVER;
const SLIVER: &str = names::MUSCLE_SLIVER;

/// "All Slivers have 'When this permanent enters, destroy target artifact or enchantment.'": a
/// triggered ability that a static ability grants. Each Sliver that enters has it as it enters,
/// Harmonic Sliver among them (CR 603.6a), and each Harmonic Sliver grants one instance of it
/// (CR 113.2c). Its controller is the controller of the Sliver that entered.
pub const GRANTED_ENTERS: &[Scenario] = &[
    Scenario::new("A granted enters trigger fires for the granting Sliver itself")
        .battlefield(P0, &[names::FOREST, names::FOREST, names::PLAINS])
        .hand(P0, &[HARMONIC])
        .battlefield(P1, &[SMALL_ARTIFACT])
        .actions(&[
            ME.casts(HARMONIC),
            Resolve,
            stack(1),
            Resolve,
            THEM.controls(SMALL_ARTIFACT, 0),
            THEM.has(Zone::Graveyard, SMALL_ARTIFACT, 1),
        ]),
    Scenario::new("A granted enters trigger fires for a Sliver that enters later")
        .interaction(&[SLIVER])
        .battlefield(P0, &[HARMONIC, names::FOREST, names::FOREST])
        .hand(P0, &[SLIVER])
        .battlefield(P1, &[SMALL_ARTIFACT])
        .actions(&[
            ME.casts(SLIVER),
            Resolve,
            stack(1),
            Resolve,
            THEM.controls(SMALL_ARTIFACT, 0),
        ]),
    Scenario::new("Two granting Slivers make a third Sliver's enters trigger fire twice")
        .interaction(&[SLIVER])
        .battlefield(P0, &[HARMONIC, HARMONIC, names::FOREST, names::FOREST])
        .hand(P0, &[SLIVER])
        .battlefield(P1, &[SMALL_ARTIFACT, names::ORNITHOPTER])
        .actions(&[
            ME.casts(SLIVER),
            Resolve,
            ME.targets(SMALL_ARTIFACT),
            ME.targets(names::ORNITHOPTER),
            stack(2),
            Resolve,
            Resolve,
            THEM.controls(SMALL_ARTIFACT, 0),
            THEM.controls(names::ORNITHOPTER, 0),
        ]),
    Scenario::new("A non-Sliver entering has no granted enters trigger")
        .battlefield(P0, &[HARMONIC, names::ISLAND, names::ISLAND])
        .hand(P0, &[props::VANILLA.card])
        .battlefield(P1, &[SMALL_ARTIFACT])
        .actions(&[
            ME.casts(props::VANILLA.card),
            Resolve,
            stack(0),
            THEM.controls(SMALL_ARTIFACT, 1),
        ]),
    Scenario::new("An opponent's Sliver entering triggers for the opponent")
        .interaction(&[SLIVER])
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(P0, &[HARMONIC, SMALL_ARTIFACT])
        .battlefield(P1, &[names::FOREST, names::FOREST, names::ORNITHOPTER])
        .hand(P1, &[SLIVER])
        .actions(&[
            THEM.casts(SLIVER),
            Resolve,
            THEM.targets(SMALL_ARTIFACT),
            Resolve,
            ME.controls(SMALL_ARTIFACT, 0),
            THEM.controls(names::ORNITHOPTER, 1),
        ]),
];
