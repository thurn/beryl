//! Transform ([CR 701.27](https://yawgatog.com/resources/magic-rules/#R70127)), and a card
//! put onto the battlefield transformed
//! ([CR 712.11a](https://yawgatog.com/resources/magic-rules/#R71211a)).

use beryl_engine::{DecisionKind, TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::{names, props};

const JACE: &str = names::JACE_VRYNS_PRODIGY;

/// A permanent that left the battlefield before the effect would exile it is a new object
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)): "An object that moves
/// from one zone to another becomes a new object with no memory of, or relation to, its
/// previous existence." It is not exiled, so it does not return transformed, though the
/// rest of the effect happens.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A card returned to hand in response is not exiled to return transformed")
            .at(1, P0, TurnStep::PrecombatMain)
            .permanents(P0, &[Permanent::new(JACE)])
            .graveyard(P0, &[names::PLAINS; 4])
            .library(P0, &[names::ISLAND, names::ISLAND])
            .permanents(P1, &[Permanent::new(names::ISLAND); 3])
            .hand(P1, &[props::REMOVAL])
            .library(P1, &[names::PLAINS])
            .actions(&[
                Action::activate(P0, JACE, 0),
                Action::cast(P1, props::REMOVAL),
                Action::Resolve,
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Hand,
                    card: Some(JACE),
                    count: 1,
                }),
                // The loot: an Island drawn, and discarded.
                Action::Resolve,
                Action::choose(P0, DecisionKind::ChooseObject, names::ISLAND),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Graveyard,
                    card: None,
                    count: 5,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Hand,
                    card: Some(JACE),
                    count: 1,
                }),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Exile,
                    card: None,
                    count: 0,
                }),
            ]),
    ];
