// Generated from the card store by `just card header`: notes go below the end marker.
//! Thirst for Discovery {2}{U}
//! Instant
//!
//! Draw three cards. Then discard two cards unless you discard a basic land card.
// End of the generated card header.

use crate::{names, printed};
use beryl_card::effect::build::{YOU, discard, draw, sequence, unless};
use beryl_card::{Ability, CardDef, Cost, CostPart, Effect, Filter, Supertypes, Types, card};

const BASIC_LAND: Filter = Filter::All(&[
    Filter::Types(Types::LAND),
    Filter::Supertypes(Supertypes::BASIC),
]);

/// "Discard two cards unless you discard a basic land card"
/// ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)): "This means the same
/// thing as '[A player may do something else]. If [that player doesn't], [do something].'"
/// So the player may discard a basic land card; if they don't, they discard two cards, basic
/// lands among them or not, or as many as they have
/// ([CR 101.3](https://yawgatog.com/resources/magic-rules/#R1013)): "Any part of an
/// instruction that's impossible to perform is ignored." With a basic land and nothing else
/// in hand, both ways discard it, so nothing is asked. A Tundra is not one, its land types
/// notwithstanding ([CR 205.4c](https://yawgatog.com/resources/magic-rules/#R2054c)): "Any
/// land that doesn't have this supertype is a nonbasic land, even if it has a basic land
/// type."
static DRAW_THREE_DISCARD: Effect = sequence(&[
    draw(YOU, 3),
    unless(
        &discard(YOU, 2),
        YOU,
        Cost::of_parts(&[CostPart::discard(BASIC_LAND)]),
    ),
]);

pub static CARD: CardDef = card(
    names::THIRST_FOR_DISCOVERY,
    &[printed::THIRST_FOR_DISCOVERY.abilities(&[Ability::spell().effect(&DRAW_THREE_DISCARD)])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use beryl_card::{DecisionKind, Seat, Zone};

    use beryl_scenario_types::{Action, Assertion, Ending, P0, P1, Permanent, Pick, Scenario};

    use crate::{names, props};

    const ISLANDS: &[Permanent] = &[Permanent::new(names::ISLAND); 3];

    const THIRST: &str = names::THIRST_FOR_DISCOVERY;

    const fn count(zone: Zone, card: Option<&'static str>, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player: P0,
            zone,
            card,
            count,
        })
    }

    const fn discard(card: &'static str) -> Action {
        Action::choose(P0, DecisionKind::ChooseObject, card)
    }

    const fn yes_no(player: Seat, pick: Pick) -> Action {
        Action::answer(player, DecisionKind::YesNo, pick)
    }

    /// P0's library, top first: three to draw, then one more so P0 doesn't lose by drawing from
    /// an empty library.
    const DRAWS: &[&str] = &[
        names::PLAINS,
        props::ARTIFACT,
        names::COUNTERSPELL,
        names::ISLAND,
    ];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Thirst for Discovery: discarding a basic land instead")
            .permanents(P0, ISLANDS)
            .hand(P0, &[THIRST])
            .library(P0, DRAWS)
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, THIRST),
                Action::Resolve,
                yes_no(P0, Pick::Yes),
                count(Zone::Graveyard, Some(names::PLAINS), 1),
                count(Zone::Hand, None, 2),
            ]),
        // Declining, P0 discards two cards, and the Plains may be one of them.
        Scenario::new("Thirst for Discovery: discarding two cards with a basic land in hand")
            .permanents(P0, ISLANDS)
            .hand(P0, &[THIRST])
            .library(P0, DRAWS)
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, THIRST),
                Action::Resolve,
                yes_no(P0, Pick::No),
                discard(props::ARTIFACT),
                discard(names::PLAINS),
                count(Zone::Graveyard, Some(props::ARTIFACT), 1),
                count(Zone::Graveyard, Some(names::PLAINS), 1),
                count(Zone::Hand, Some(names::COUNTERSPELL), 1),
            ]),
        // Tundra is a Plains Island but not basic: no choice, two discards.
        Scenario::new("Thirst for Discovery: two nonbasic cards with no basic land")
            .permanents(P0, ISLANDS)
            .hand(P0, &[THIRST])
            .library(
                P0,
                &[
                    names::TUNDRA,
                    props::ARTIFACT,
                    names::COUNTERSPELL,
                    names::ISLAND,
                ],
            )
            .library(P1, &[names::ISLAND])
            .actions(&[
                Action::cast(P0, THIRST),
                Action::Resolve,
                discard(names::TUNDRA),
                discard(names::COUNTERSPELL),
                count(Zone::Hand, Some(props::ARTIFACT), 1),
                count(Zone::Graveyard, None, 3),
            ]),
        // One card to draw: P0 draws it, discards it, the only card in hand, and then loses for
        // having drawn from an empty library
        // ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)).
        Scenario::new("Thirst for Discovery with fewer than two cards to discard")
            .permanents(P0, ISLANDS)
            .hand(P0, &[THIRST])
            .library(P0, &[props::ARTIFACT])
            .library(P1, &[names::ISLAND])
            .actions(&[
                // P0 has nothing else to do, so Thirst resolves at once.
                Action::cast(P0, THIRST),
                count(Zone::Graveyard, Some(props::ARTIFACT), 1),
                count(Zone::Hand, None, 0),
                Action::assert(Assertion::Outcome {
                    outcome: Ending::Win { player: P1 },
                }),
            ]),
    ];
}
