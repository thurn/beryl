//! Search ([CR 701.23](https://yawgatog.com/resources/magic-rules/#R70123)).

use beryl_engine::{DecisionKind, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};
use beryl_scenario_types::{ME, Resolve};

use crate::{names, props};

const WINDS: &str = names::WINDS_OF_ABANDON;

/// A player searching a hidden zone may find nothing though a card of the kind is there
/// ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b)): "that player isn't
/// required to find some or all of those cards even if they're present in that zone." The
/// player whose creature was exiled searches, finds no basic land and puts none onto the
/// battlefield; their library still holds its three cards.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("A player searching for a basic land card finds none though one is there")
            .permanents(
                P0,
                &[Permanent::new(names::PLAINS), Permanent::new(names::PLAINS)],
            )
            .hand(P0, &[WINDS, names::PLAINS])
            .permanents(P1, &[Permanent::new(props::FLIER.card)])
            .library(P1, &[names::TUNDRA, names::PLAINS, names::ISLAND])
            .actions(&[
                Action::cast(P0, WINDS),
                Action::Resolve,
                Action::answer(P1, DecisionKind::ChooseUpTo, Pick::Done),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Exile,
                    card: Some(props::FLIER.card),
                    count: 1,
                }),
                Action::assert(Assertion::Controls {
                    player: P1,
                    card: names::PLAINS,
                    count: 0,
                }),
                Action::assert(Assertion::Count {
                    player: P1,
                    zone: Zone::Library,
                    card: None,
                    count: 3,
                }),
            ]),
    ];

/// A successful search reveals the chosen card before moving it to hand. The session
/// tests replay the cast and search one step at a time.
pub const FIND_AND_REVEAL: &[Scenario] = &[Scenario::new("Trinket Mage finds Portable Hole")
    .hand(P0, &[names::TRINKET_MAGE])
    .battlefield(P0, &[names::ISLAND; 3])
    .library(
        P0,
        &[
            names::ISLAND,
            props::ARTIFACT.card,
            names::OUST,
            names::PLAINS,
        ],
    )
    .library(P1, &[names::PLAINS])
    .actions(&[
        ME.casts(names::TRINKET_MAGE),
        Resolve,
        Resolve,
        ME.yes(),
        // Finding nothing or the Hole.
        ME.deciding(DecisionKind::ChooseUpTo, 2),
        ME.picks_up_to(names::PORTABLE_HOLE),
        ME.has(Zone::Hand, names::PORTABLE_HOLE, 1),
        ME.has_cards(Zone::Library, 3),
    ])];
