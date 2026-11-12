//! Shuffle ([CR 701.24](https://yawgatog.com/resources/magic-rules/#R70124)).

use beryl_engine::{Seat, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

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
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("Each player's hand and graveyard are shuffled into their own library")
            .battlefield(P0, &[Permanent::new(names::ISLAND); 3])
            .hand(P0, &[names::TIMETWISTER, names::PLAINS])
            .graveyard(P0, &[props::SORCERY])
            .library(P0, &[names::PLAINS; 7])
            .hand(P1, &[names::ISLAND])
            .graveyard(P1, &[props::INSTANT, props::SORCERY])
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
    ];
