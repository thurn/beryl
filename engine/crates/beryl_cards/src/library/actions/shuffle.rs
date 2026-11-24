//! Shuffle ([CR 701.24](https://yawgatog.com/resources/magic-rules/#R70124)).

use beryl_engine::{Seat, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};
use beryl_scenario_types::{ME, THEM};

use crate::{names, props};

const fn count(player: Seat, zone: Zone, count: u8) -> Action {
    Action::assert(Assertion::Count {
        player,
        zone,
        card: None,
        count,
    })
}

/// Cards shuffled into a library go to their owners' libraries
/// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object would go to
/// any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone." Each player's hand and graveyard join their own library: nine cards
/// for each, of which they draw seven. The spell that did it is in its owner's graveyard.
pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("Each player's hand and graveyard are shuffled into their own library")
        .permanents(P0, &[Permanent::new(names::ISLAND); 3])
        .hand(P0, &[names::TIMETWISTER, names::PLAINS])
        .graveyard(P0, &[props::SORCERY.card])
        .library(P0, &[names::PLAINS; 7])
        .hand(P1, &[names::ISLAND])
        .graveyard(P1, &[props::INSTANT.card, props::SORCERY.card])
        .library(P1, &[names::ISLAND; 6])
        .actions(&[
            Action::cast(P0, names::TIMETWISTER),
            count(P0, Zone::Hand, 7),
            count(P0, Zone::Library, 2),
            count(P0, Zone::Graveyard, 1),
            count(P1, Zone::Hand, 7),
            count(P1, Zone::Library, 2),
            count(P1, Zone::Graveyard, 0),
        ]),
    // With empty graveyards, only the cards in hand are visibly moved into the libraries.
    Scenario::new("Timetwister with empty graveyards")
        .battlefield(P0, &[names::ISLAND; 3])
        .hand(P0, &[names::TIMETWISTER, names::PLAINS])
        .library(P0, &[names::PLAINS; 7])
        .hand(P1, &[names::ISLAND, names::ISLAND])
        .library(P1, &[names::ISLAND; 6])
        .actions(&[
            ME.casts(names::TIMETWISTER),
            ME.has_cards(Zone::Hand, 7),
            ME.has_cards(Zone::Library, 1),
            ME.has(Zone::Graveyard, names::TIMETWISTER, 1),
            ME.has_cards(Zone::Graveyard, 1),
            THEM.has_cards(Zone::Hand, 7),
            THEM.has_cards(Zone::Library, 1),
        ]),
];
