//! Tokens ([CR 111](https://yawgatog.com/resources/magic-rules/#R111)).

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::tokens::HUMAN_TOKEN;
use crate::{names, props};

const ADELINE: &str = names::ADELINE_RESPLENDENT_CATHAR;

/// A token put onto the battlefield attacking deals combat damage with the creatures that
/// were declared ([CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)): "If a
/// creature is put onto the battlefield attacking, its controller chooses which defending
/// player, planeswalker a defending player controls, or battle a defending player protects
/// it's attacking as it enters the battlefield". With it there are two creatures, so two
/// damage from the one declared and one from the token.
pub const ATTACKING: &[Scenario] =
    &[
        Scenario::new("A token created attacking deals combat damage")
            .permanents(P0, &[Permanent::new(ADELINE)])
            .library(P0, &[names::PLAINS])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P0),
                Action::choose(P0, DecisionKind::Attacker, ADELINE),
                Action::Resolve,
                Action::assert(Assertion::Attacking {
                    player: P0,
                    card: HUMAN_TOKEN,
                    count: 1,
                }),
                Action::AdvanceTo {
                    turn: 1,
                    step: TurnStep::CombatDamage,
                },
                Action::assert(Assertion::Life {
                    player: P1,
                    life: 17,
                }),
            ]),
    ];

/// A copy of a permanent the effect exiled is of the permanent as it last was, and each
/// player who creates one owns theirs
/// ([CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112)): "The player who creates
/// a token is its owner. The token enters the battlefield under that player's control." The
/// permanent's controller creates none.
pub const COPIES: &[Scenario] =
    &[
        Scenario::new(
            "A copy of an exiled permanent is made by the players who did not control it",
        )
        .permanents(
            P0,
            &[
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
                Permanent::new(names::PLAINS),
                Permanent::new(names::ISLAND),
                Permanent::new(names::ISLAND),
            ],
        )
        .hand(P0, &[names::FRACTURED_IDENTITY])
        .library(P0, &[names::PLAINS])
        .permanents(P1, &[Permanent::new(props::ARTIFACT)])
        .library(P1, &[names::PLAINS])
        .actions(&[
            Action::cast(P0, names::FRACTURED_IDENTITY),
            Action::Resolve,
            Action::assert(Assertion::Count {
                player: P1,
                zone: Zone::Exile,
                card: Some(props::ARTIFACT),
                count: 1,
            }),
            Action::assert(Assertion::Controls {
                player: P0,
                card: props::ARTIFACT,
                count: 1,
            }),
            Action::assert(Assertion::Controls {
                player: P1,
                card: props::ARTIFACT,
                count: 0,
            }),
        ]),
    ];
