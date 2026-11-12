//! Manifest dread ([CR 701.62](https://yawgatog.com/resources/magic-rules/#R70162)).

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::{names, props};

/// Either card may be the one manifested, a noncreature card too, and the other goes to the
/// graveyard ([CR 701.62a](https://yawgatog.com/resources/magic-rules/#R70162a)): "Manifest
/// one of them, then put the cards you looked at that were not manifested this way into your
/// graveyard."
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("Manifest dread manifests the instant and buries the creature")
            .at(1, P0, TurnStep::PostcombatMain)
            .battlefield(P0, &[Permanent::new(names::ABHORRENT_OCULUS)])
            .library(P0, &[props::SMALL_CREATURE, props::INSTANT, names::ISLAND])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::Upkeep,
                },
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, props::INSTANT),
                Action::assert(Assertion::FaceDown {
                    player: P0,
                    card: props::INSTANT,
                    count: 1,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: Some(props::SMALL_CREATURE),
                    count: 1,
                }),
                Action::assert(Assertion::Library {
                    player: P0,
                    top: &[names::ISLAND],
                }),
            ]),
    ];
