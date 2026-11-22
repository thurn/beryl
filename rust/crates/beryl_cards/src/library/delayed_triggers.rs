//! Delayed triggered abilities
//! ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).

use beryl_engine::card::{Castings, FaceIx, ManaColor};
use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve, THEM, advance_to, stack};

use crate::{names, props};

const TEFERI: &str = names::TEFERI_HERO_OF_DOMINARIA;

/// A delayed ability an effect made triggers once
/// ([CR 603.7b](https://yawgatog.com/resources/magic-rules/#R6037b)): "A delayed triggered
/// ability will trigger only once--the next time its trigger event occurs--unless it has a
/// stated duration". Its controller taps two Plains for two artifacts and untaps one of them
/// at the first end step. At the second nothing triggers: the other Plains is still tapped
/// and the stack is empty.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A delayed ability triggers at the next end step and at no later one")
            .permanents(
                P0,
                &[
                    Permanent::new(TEFERI),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[props::ARTIFACT.card, props::ARTIFACT.card])
            .library(P0, &[names::ISLAND, names::ISLAND, names::ISLAND])
            .permanents(P1, &[Permanent::new(names::ISLAND)])
            .library(P1, &[names::ISLAND, names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT.card),
                Action::Resolve,
                Action::cast(P0, props::ARTIFACT.card),
                Action::Resolve,
                Action::activate(P0, TEFERI, 0),
                Action::Resolve,
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::End,
                },
                Action::assert(Assertion::Stack { count: 1 }),
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseUpTo, names::PLAINS),
                Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::End,
                },
                Action::assert(Assertion::Stack { count: 0 }),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::PLAINS,
                    count: 1,
                }),
            ]),
    ];

const DRAIN: &str = names::MANA_DRAIN;
const WISP: &str = names::FLICKERWISP;
const SKYDIVER: &str = props::THIEF.card;
const MYR: &str = names::GOLD_MYR;

/// The delayed mana ability resolves at the caster's first subsequent main phase, then
/// leaves no ability on the stack at the caster's next turn.
pub const NEXT_MAIN_PHASE: &[Scenario] =
    &[
        Scenario::new("Mana Drain adds mana at only the next main phase")
            .battlefield(P0, &[names::ISLAND, names::ISLAND, names::PLAINS])
            .hand(P0, &[props::ARTIFACT.card, DRAIN])
            .library(P0, &[names::ISLAND; 2])
            .battlefield(P1, &[names::ISLAND; 2])
            .hand(P1, &[props::INSTANT.card])
            .actions(&[
                ME.casts(props::ARTIFACT.card),
                ME.passes(),
                THEM.casts(props::INSTANT.card),
                ME.casts(DRAIN),
                ME.targets(props::INSTANT.card),
                Resolve,
                Resolve,
                ME.mana(ManaColor::Colorless, 0),
                advance_to(1, TurnStep::PostcombatMain),
                stack(1),
                Resolve,
                ME.mana(ManaColor::Colorless, 2),
                advance_to(3, TurnStep::PrecombatMain),
                stack(0),
                ME.mana(ManaColor::Colorless, 0),
            ]),
    ];

/// The exiled object is no longer affected by the effect that stole it. It returns
/// untapped under its owner's control at the beginning of the end step.
pub const RETURN_TO_OWNER: &[Scenario] =
    &[
        Scenario::new("The delayed return gives a stolen creature back untapped")
            .interaction(&[MYR])
            .battlefield(
                P0,
                &[
                    names::ISLAND,
                    names::PLAINS,
                    names::PLAINS,
                    names::PLAINS,
                    names::PLAINS,
                    names::PLAINS,
                    names::PLAINS,
                ],
            )
            .permanents(P1, &[Permanent::new(MYR).tapped()])
            .hand(P0, &[SKYDIVER, WISP])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: SKYDIVER,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.x(2),
                Resolve,
                Resolve,
                ME.controls(MYR, 1),
                ME.tapped(MYR, 1),
                ME.casts(WISP),
                Resolve,
                ME.targets(MYR),
                Resolve,
                THEM.has(Zone::Exile, MYR, 1),
                ME.controls(MYR, 0),
                THEM.controls(MYR, 0),
                advance_to(1, TurnStep::End),
                stack(1),
                Resolve,
                THEM.controls(MYR, 1),
                THEM.tapped(MYR, 0),
                ME.controls(MYR, 0),
                THEM.has(Zone::Exile, MYR, 0),
            ]),
    ];
