// Generated from the card store by `just card header`: notes go below the end marker.
//! Time Spiral {4}{U}{U}
//! Sorcery
//!
//! Exile Time Spiral. Each player shuffles their hand and graveyard into their library, then draws
//! seven cards. You untap up to six lands.
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{
    EACH_PLAYER, THIS, YOU, draw, exile, sequence, shuffle_into, untap_up_to,
};
use beryl_card::{Ability, CardDef, Effect, Filter, Objects, Types, Zone, card};

/// The instructions in order ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)):
/// "The controller of the spell or ability follows its instructions in the order written."
/// Exiled first, Time Spiral is not shuffled away, and it is not put into a graveyard as the
/// final part of its resolution (CR 608.2n): it is no longer on the stack.
///
/// The hands and graveyards go to their libraries together, each card to its owner's
/// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)), each library is shuffled
/// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)), and each player
/// draws seven in turn order ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)).
///
/// "Up to six lands" are any lands, whoever controls them.
static SPIRAL: Effect = sequence(&[
    exile(THIS),
    shuffle_into(
        EACH_PLAYER,
        Objects::CardsIn {
            players: &EACH_PLAYER,
            zones: &[Zone::Hand, Zone::Graveyard],
        },
    ),
    draw(EACH_PLAYER, 7),
    untap_up_to(
        YOU,
        6,
        Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Types(Types::LAND),
        },
    ),
]);

pub static CARD: CardDef = card(
    names::TIME_SPIRAL,
    &[printed::TIME_SPIRAL.abilities(&[Ability::spell().effect(&SPIRAL)])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::DecisionKind;
    use beryl_card::Seat;
    use beryl_card::Zone;

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

    use crate::names;

    /// Six lands to cast it with, all tapped by the casting.
    const SIX_LANDS: &[Permanent] = &[
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
        Permanent::new(names::PLAINS),
    ];

    const fn count(player: Seat, zone: Zone, card: Option<&'static str>, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player,
            zone,
            card,
            count,
        })
    }

    const fn tapped(player: Seat, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Tapped {
            player,
            card,
            count,
        })
    }

    const fn untap(card: &'static str) -> Action {
        Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Card { card })
    }

    /// P1's tapped Tundra is a land P0 may untap too. Interchangeable lands are one choice each
    /// time: after two Islands no Island is left to offer.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Time Spiral: exiled, the wheel, and chosen lands untapped")
            .battlefield(P0, SIX_LANDS)
            .hand(P0, &[names::TIME_SPIRAL, names::ISLAND])
            .graveyard(P0, &[names::PLAINS])
            .library(P0, &[names::ISLAND; 6])
            .battlefield(P1, &[Permanent::new(names::TUNDRA).tapped()])
            .hand(P1, &[names::PLAINS])
            .graveyard(P1, &[names::ISLAND])
            .library(P1, &[names::PLAINS; 6])
            .actions(&[
                Action::cast(P0, names::TIME_SPIRAL),
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ChooseUpTo,
                    candidates: 4,
                }),
                untap(names::ISLAND),
                untap(names::ISLAND),
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ChooseUpTo,
                    candidates: 3,
                }),
                untap(names::PLAINS),
                Action::answer(P0, DecisionKind::ChooseUpTo, Pick::Done),
                tapped(P0, names::ISLAND, 0),
                tapped(P0, names::PLAINS, 3),
                tapped(P1, names::TUNDRA, 1),
                count(P0, Zone::Exile, Some(names::TIME_SPIRAL), 1),
                count(P0, Zone::Graveyard, None, 0),
                count(P0, Zone::Hand, None, 7),
                count(P0, Zone::Library, None, 1),
                count(P1, Zone::Graveyard, None, 0),
                count(P1, Zone::Hand, None, 7),
                count(P1, Zone::Library, None, 1),
            ]),
        Scenario::new("Time Spiral with empty graveyards: six lands untapped, any player's")
            .battlefield(P0, SIX_LANDS)
            .hand(P0, &[names::TIME_SPIRAL])
            .library(P0, &[names::ISLAND; 7])
            .battlefield(P1, &[Permanent::new(names::TUNDRA).tapped()])
            .library(P1, &[names::PLAINS; 7])
            .actions(&[
                Action::cast(P0, names::TIME_SPIRAL),
                untap(names::TUNDRA),
                untap(names::PLAINS),
                untap(names::PLAINS),
                untap(names::PLAINS),
                untap(names::PLAINS),
                untap(names::ISLAND),
                // Six chosen: no more is asked.
                tapped(P0, names::ISLAND, 1),
                tapped(P0, names::PLAINS, 0),
                tapped(P1, names::TUNDRA, 0),
                count(P0, Zone::Graveyard, None, 0),
                count(P0, Zone::Hand, None, 7),
                count(P1, Zone::Hand, None, 7),
            ]),
    ];
}
