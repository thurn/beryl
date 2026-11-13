//! Costs ([CR 118](https://yawgatog.com/resources/magic-rules/#R118)) paid while a spell or
//! ability resolves.

use beryl_engine::{DecisionKind, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use crate::{names, props};

const THIRST: &str = names::THIRST_FOR_DISCOVERY;

const fn count(zone: Zone, card: Option<&'static str>, count: u8) -> Action {
    Action::assert(Assertion::Count {
        player: P0,
        zone,
        card,
        count,
    })
}

/// A cost that is an action is the player's to pay with the card they choose
/// ([CR 118.12](https://yawgatog.com/resources/magic-rules/#R11812)): "The action [do
/// something] is a cost, paid when the spell or ability resolves." With two basic land cards
/// in hand, a yes is followed by which of them to discard, and the other effect, the discard
/// of two cards, does not happen.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A discard paid as a cost is of the card its player chooses")
            .permanents(P0, &[Permanent::new(names::ISLAND); 3])
            .hand(P0, &[THIRST])
            .library(
                P0,
                &[names::PLAINS, names::ISLAND, props::ARTIFACT, names::ISLAND],
            )
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, THIRST),
                Action::Resolve,
                Action::answer(P0, DecisionKind::YesNo, Pick::Yes),
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ChooseObject,
                    candidates: 2,
                }),
                Action::choose(P0, DecisionKind::ChooseObject, names::ISLAND),
                count(Zone::Graveyard, Some(names::ISLAND), 1),
                count(Zone::Hand, Some(names::PLAINS), 1),
                count(Zone::Hand, None, 2),
            ]),
    ];
