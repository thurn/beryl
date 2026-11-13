// Generated from the card store by `just card header`: notes go below the end marker.
//! Stock Up {2}{U}
//! Sorcery
//!
//! Look at the top five cards of your library. Put two of them into your hand and the rest on the
//! bottom of your library in any order.
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{YOU, dig};
use beryl_card::{Ability, CardDef, Effect, LibraryOrder, Taken, card};

/// The two for the hand are chosen one at a time and go there together; with two or fewer
/// cards left to choose from, they all go unasked. The rest go to the bottom one at a time,
/// each under the ones before, which is every order
/// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "If an effect puts two or
/// more cards in a specific position in a library at the same time, the owner of those cards
/// may arrange them in any order." With fewer than five cards in the library, the player
/// looks at them all.
static STOCK_UP: Effect = dig(YOU, 5, Taken::Put(2), LibraryOrder::Any);

pub static CARD: CardDef = card(
    names::STOCK_UP,
    &[printed::STOCK_UP.abilities(&[Ability::spell().effect(&STOCK_UP)])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::{DecisionKind, Zone};

    use beryl_scenario_types::{Action, Assertion, P0, P1, Permanent, Scenario};

    use crate::{names, props};

    const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 3];

    const fn in_hand(card: &'static str) -> Action {
        Action::assert(Assertion::Count {
            player: P0,
            zone: Zone::Hand,
            card: Some(card),
            count: 1,
        })
    }

    const fn take(card: &'static str) -> Action {
        Action::choose(P0, DecisionKind::ChooseObject, card)
    }

    pub const SCENARIOS: &[Scenario] = &[
        // The artifact and Counterspell to the hand; Oust, then Plains, then Island to the
        // bottom, each under the one before. The sixth card, Tundra, is now on top.
        Scenario::new("Stock Up takes two of five and orders the rest on the bottom")
            .battlefield(P0, ISLANDS)
            .hand(P0, &[names::STOCK_UP])
            .library(
                P0,
                &[
                    names::ISLAND,
                    names::PLAINS,
                    props::ARTIFACT,
                    names::COUNTERSPELL,
                    props::SORCERY,
                    names::TUNDRA,
                ],
            )
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::STOCK_UP),
                Action::Resolve,
                Action::assert(Assertion::Decision {
                    player: P0,
                    kind: DecisionKind::ChooseObject,
                    candidates: 5,
                }),
                take(props::ARTIFACT),
                take(names::COUNTERSPELL),
                take(props::SORCERY),
                take(names::PLAINS),
                in_hand(props::ARTIFACT),
                in_hand(names::COUNTERSPELL),
                Action::assert(Assertion::Library {
                    player: P0,
                    top: &[names::TUNDRA, props::SORCERY, names::PLAINS, names::ISLAND],
                }),
            ]),
        // Four cards: two to the hand, and the order of the other two on the bottom.
        Scenario::new("Stock Up with four cards in the library")
            .battlefield(P0, ISLANDS)
            .hand(P0, &[names::STOCK_UP])
            .library(
                P0,
                &[
                    names::PLAINS,
                    props::ARTIFACT,
                    names::ISLAND,
                    names::COUNTERSPELL,
                ],
            )
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::STOCK_UP),
                Action::Resolve,
                take(props::ARTIFACT),
                take(names::COUNTERSPELL),
                take(names::ISLAND),
                in_hand(props::ARTIFACT),
                in_hand(names::COUNTERSPELL),
                Action::assert(Assertion::Library {
                    player: P0,
                    top: &[names::ISLAND, names::PLAINS],
                }),
            ]),
        // Two cards: both go to the hand, with nothing to choose.
        Scenario::new("Stock Up with two cards in the library")
            .battlefield(P0, ISLANDS)
            .hand(P0, &[names::STOCK_UP])
            .library(P0, &[props::ARTIFACT, names::PLAINS])
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, names::STOCK_UP),
                Action::Resolve,
                in_hand(props::ARTIFACT),
                in_hand(names::PLAINS),
                Action::assert(Assertion::Count {
                    player: P0,
                    zone: Zone::Library,
                    card: None,
                    count: 0,
                }),
            ]),
    ];
}
