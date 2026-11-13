// Generated from the card store by `just card header`: notes go below the end marker.
//! Timetwister {2}{U}
//! Sorcery
//!
//! Each player shuffles their hand and graveyard into their library, then draws seven cards. (Then
//! put Timetwister into its owner's graveyard.)
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{EACH_PLAYER, draw, sequence, shuffle_into};
use beryl_card::{Ability, CardDef, Effect, Objects, Zone, card};

/// "Each player shuffles their hand and graveyard into their library, then draws seven
/// cards." The cards go to their libraries together, each to its owner's
/// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)), and each library is
/// shuffled ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)). Then each
/// player draws seven, one card at a time
/// ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)), in APNAP order
/// ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)): "the active player ...
/// makes any choices required, then the next player in turn order ... Then the actions happen
/// simultaneously." No draw involves a choice, so the order changes nothing: a player who draws
/// from an empty library loses as state-based actions are next checked either way (CR 704.5b).
static TWIST: Effect = sequence(&[
    shuffle_into(
        EACH_PLAYER,
        Objects::CardsIn {
            players: &EACH_PLAYER,
            zones: &[Zone::Hand, Zone::Graveyard],
        },
    ),
    draw(EACH_PLAYER, 7),
]);

pub static CARD: CardDef = card(
    names::TIMETWISTER,
    &[printed::TIMETWISTER.abilities(&[Ability::spell().effect(&TWIST)])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::{Seat, Zone};
    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::{names, props};

    const fn count(player: Seat, zone: Zone, card: Option<&'static str>, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card,
            count,
        })
    }

    const THREE_ISLANDS: &[Permanent] = &[
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
    ];

    /// Each player ends with seven cards in hand and the rest of hand, graveyard and library in
    /// the library; Timetwister alone goes to the graveyard, after the shuffle.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Timetwister with empty graveyards")
            .battlefield(P0, THREE_ISLANDS)
            .hand(P0, &[names::TIMETWISTER, names::PLAINS])
            .library(P0, &[names::PLAINS; 7])
            .hand(P1, &[names::ISLAND, names::ISLAND])
            .library(P1, &[names::ISLAND; 6])
            .actions(&[
                Action::cast(P0, names::TIMETWISTER),
                count(P0, Zone::Hand, None, 7),
                count(P0, Zone::Library, None, 1),
                count(P0, Zone::Graveyard, Some(names::TIMETWISTER), 1),
                count(P0, Zone::Graveyard, None, 1),
                count(P1, Zone::Hand, None, 7),
                count(P1, Zone::Library, None, 1),
            ]),
        Scenario::new("Timetwister shuffles graveyards away")
            .battlefield(P0, THREE_ISLANDS)
            .hand(P0, &[names::TIMETWISTER])
            .graveyard(P0, &[props::INSTANT, names::OUST])
            .library(P0, &[names::PLAINS; 5])
            .graveyard(P1, &[props::INSTANT])
            .library(P1, &[names::ISLAND; 8])
            .actions(&[
                Action::cast(P0, names::TIMETWISTER),
                count(P0, Zone::Hand, None, 7),
                count(P0, Zone::Library, None, 0),
                count(P0, Zone::Graveyard, None, 1),
                count(P1, Zone::Hand, None, 7),
                count(P1, Zone::Graveyard, None, 0),
                count(P1, Zone::Library, None, 2),
            ]),
    ];
}
