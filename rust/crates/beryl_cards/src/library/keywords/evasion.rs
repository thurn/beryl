//! Flying, reach and vigilance on cards complete from the card store, which have no file: their
//! definitions are their printed faces with the keywords their Oracle text lists
//! (`beryl_card_build`'s `complete`). The scenario plays two of them by name, so the oracles
//! check that such a card is a card like any other.

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::names;

/// A 4/4 with flying and vigilance, complete from the store.
pub const FLYER: &str = names::SERRA_ANGEL;

/// A vanilla 2/1, complete from the store.
pub const VANILLA: &str = names::SAVANNAH_LIONS;

/// The flyer attacks over the vanilla creature, which can't block it
/// ([CR 702.9b](https://yawgatog.com/resources/magic-rules/#R7029b)): "A creature with flying
/// can't be blocked except by creatures with flying and/or reach." The flyer stays untapped
/// ([CR 702.20b](https://yawgatog.com/resources/magic-rules/#R70220b)): "Attacking doesn't
/// cause creatures with vigilance to tap." So it blocks the vanilla creature in the next turn
/// and kills it.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A flyer and a vanilla creature from the store are cast, attack and block")
            .battlefield(
                P0,
                &[
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                    Permanent::new(names::PLAINS),
                ],
            )
            .battlefield(P1, &[Permanent::new(names::PLAINS)])
            .hand(P0, &[FLYER])
            .hand(P1, &[VANILLA])
            .library(P0, &[names::PLAINS, names::PLAINS])
            .library(P1, &[names::PLAINS, names::PLAINS])
            .actions(&[
                Action::cast(P0, FLYER),
                Action::Resolve,
                Action::AdvanceTo {
                    turn: 2,
                    step: TurnStep::PrecombatMain,
                },
                Action::cast(P1, VANILLA),
                Action::Resolve,
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P0),
                Action::choose(P0, DecisionKind::Attacker, FLYER),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::EndOfCombat,
                },
                Action::assert(Assertion::Life {
                    player: P1,
                    life: 16,
                }),
                Action::assert(Assertion::Tapped {
                    player: P0,
                    card: FLYER,
                    count: 0,
                }),
                Action::AdvanceTo {
                    turn: 4,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::pass(P1),
                Action::choose(P1, DecisionKind::Attacker, VANILLA),
                Action::choose(P0, DecisionKind::Blocker, FLYER),
                Action::AdvanceTo {
                    turn: 4,
                    step: TurnStep::EndOfCombat,
                },
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Graveyard,
                    card: Some(VANILLA),
                    count: 1,
                }),
                Action::assert(Assertion::Life {
                    player: P0,
                    life: 20,
                }),
            ]),
    ];
