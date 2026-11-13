// Generated from the card store by `just card header`: notes go below the end marker.
//! Thirst for Discovery {2}{U}
//! Instant
//!
//! Draw three cards. Then discard two cards unless you discard a basic land card.
// End of the generated card header.

use crate::prelude::*;

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
pub static CARD: CardDef = Printed::ThirstForDiscovery
    .abilities(&[Ability::spell().effect(&sequence(&[
        draw(you(), 3),
        unless(
            &discard(you(), 2),
            you(),
            Cost::part(&CostPart::discard(Filter::All(&[
                Filter::Types(Types::LAND),
                Filter::Supertypes(Supertypes::BASIC),
            ]))),
        ),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const THIRST: &str = Card::ThirstForDiscovery;

    /// P0's library, top first: three to draw, then one more so P0 doesn't lose by drawing from
    /// an empty library.
    const DRAWS: &[&str] = &[
        Card::Plains,
        Prop::Artifact,
        Card::Counterspell,
        Card::Island,
    ];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Thirst for Discovery: discarding a basic land instead")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[THIRST])
            .library(P0, DRAWS)
            .actions(&[
                ME.casts(THIRST),
                Resolve,
                ME.yes(),
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has_cards(Zone::Hand, 2),
            ]),
        // Declining, P0 discards two cards, and the Plains may be one of them.
        Scenario::new("Thirst for Discovery: discarding two cards with a basic land in hand")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[THIRST])
            .library(P0, DRAWS)
            .actions(&[
                ME.casts(THIRST),
                Resolve,
                ME.no(),
                ME.picks(Prop::Artifact),
                ME.picks(Card::Plains),
                ME.has(Zone::Graveyard, Prop::Artifact, 1),
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has(Zone::Hand, Card::Counterspell, 1),
            ]),
        // Tundra is a Plains Island but not basic: no choice, two discards.
        Scenario::new("Thirst for Discovery: two nonbasic cards with no basic land")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[THIRST])
            .library(
                P0,
                &[
                    Card::Tundra,
                    Prop::Artifact,
                    Card::Counterspell,
                    Card::Island,
                ],
            )
            .actions(&[
                ME.casts(THIRST),
                Resolve,
                ME.picks(Card::Tundra),
                ME.picks(Card::Counterspell),
                ME.has(Zone::Hand, Prop::Artifact, 1),
                ME.has_cards(Zone::Graveyard, 3),
            ]),
        // One card to draw: P0 draws it, discards it, the only card in hand, and then loses for
        // having drawn from an empty library
        // ([CR 704.5b](https://yawgatog.com/resources/magic-rules/#R7045b)).
        Scenario::new("Thirst for Discovery with fewer than two cards to discard")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[THIRST])
            .library(P0, &[Prop::Artifact])
            .actions(&[
                // P0 has nothing else to do, so Thirst resolves at once.
                ME.casts(THIRST),
                ME.has(Zone::Graveyard, Prop::Artifact, 1),
                ME.has_cards(Zone::Hand, 0),
                wins(THEM),
            ]),
    ];
}
