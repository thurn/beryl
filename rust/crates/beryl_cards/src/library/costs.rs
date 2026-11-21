//! Costs ([CR 118](https://yawgatog.com/resources/magic-rules/#R118)) paid to cast a spell,
//! activate an ability, or carry out an instruction during resolution.

use beryl_engine::{DecisionKind, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve};

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
                &[
                    names::PLAINS,
                    names::ISLAND,
                    props::ARTIFACT.card,
                    names::ISLAND,
                ],
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

/// The discard happens during casting. [CR 601.2h] "The player pays the total cost."
///
/// [CR 601.2h]: https://yawgatog.com/resources/magic-rules/#R6012h
pub const DISCARD_TO_CAST: &[Scenario] =
    &[
        Scenario::new("discarding a card pays for drawing two cards")
            .battlefield(P0, &[names::MOUNTAIN; 2])
            .hand(P0, &[names::THRILL_OF_POSSIBILITY, names::SWAMP])
            .library(P0, &[names::ISLAND; 3])
            .actions(&[
                ME.casts(names::THRILL_OF_POSSIBILITY),
                ME.has(Zone::Graveyard, names::SWAMP, 1),
                ME.has(Zone::Hand, names::SWAMP, 0),
                Resolve,
                ME.has(Zone::Hand, names::ISLAND, 2),
            ]),
    ];

/// The sacrifice happens during casting. [CR 601.2h] "The player pays the total cost."
///
/// [CR 601.2h]: https://yawgatog.com/resources/magic-rules/#R6012h
pub const SACRIFICE_TO_CAST: &[Scenario] =
    &[
        Scenario::new("sacrificing a creature pays for drawing two cards")
            .battlefield(P0, &[names::SWAMP, props::VANILLA.card])
            .hand(P0, &[names::VILLAGE_RITES])
            .library(P0, &[names::ISLAND; 3])
            .actions(&[
                ME.casts(names::VILLAGE_RITES),
                ME.has(Zone::Graveyard, props::VANILLA.card, 1),
                ME.controls(props::VANILLA.card, 0),
                Resolve,
                ME.has(Zone::Hand, names::ISLAND, 2),
            ]),
    ];

/// [CR 702.29a] “Cycling [cost]” means “[Cost], Discard this card: Draw a card.”
///
/// [CR 702.29a]: https://yawgatog.com/resources/magic-rules/#R70229a
pub const CYCLING: &[Scenario] = &[Scenario::new("Cycling Spara's Headquarters")
    .battlefield(P0, &[names::PLAINS; 3])
    .hand(P0, &[names::SPARAS_HEADQUARTERS])
    .actions(&[
        ME.activates(names::SPARAS_HEADQUARTERS, 4),
        ME.has(Zone::Hand, names::ISLAND, 1),
        ME.has(Zone::Graveyard, names::SPARAS_HEADQUARTERS, 1),
        ME.tapped(names::PLAINS, 3),
    ])];
