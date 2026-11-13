//! Tap and untap ([CR 701.26](https://yawgatog.com/resources/magic-rules/#R70126)).

use beryl_engine::{DecisionKind, TurnStep};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::{names, props};

const TEFERI: &str = names::TEFERI_HERO_OF_DOMINARIA;

/// "Untap up to two lands" offers the tapped lands, whoever controls them, and no untapped
/// one ([CR 701.26b](https://yawgatog.com/resources/magic-rules/#R70126b)): "Only tapped
/// permanents can be untapped." The player taps two Plains of three for two artifacts. The
/// three answers are then to stop, a tapped Plains (the two are one answer) and the
/// opponent's tapped Island. "Up to" lets the player stop at one: a Plains stays tapped.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("Untapping up to two lands offers every tapped land and no other")
            .permanents(
                P0,
                &[
                    Permanent::new(TEFERI),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                ],
            )
            .hand(P0, &[props::ARTIFACT, props::ARTIFACT])
            .library(P0, &[names::ISLAND, names::ISLAND])
            .permanents(P1, &[Permanent::new(names::ISLAND).tapped()])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, props::ARTIFACT),
                Action::Resolve,
                Action::cast(P0, props::ARTIFACT),
                Action::Resolve,
                Action::activate(P0, TEFERI, 0),
                Action::Resolve,
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::End,
                },
                Action::Resolve,
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ChooseUpTo,
                    candidates: 3,
                }),
                Action::choose(P0, DecisionKind::ChooseUpTo, names::PLAINS),
                Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: names::PLAINS,
                    count: 1,
                }),
            ]),
    ];
