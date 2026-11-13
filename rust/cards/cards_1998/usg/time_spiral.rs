// Generated from the card store by `just card header`: notes go below the end marker.
//! Time Spiral {4}{U}{U}
//! Sorcery
//!
//! Exile Time Spiral. Each player shuffles their hand and graveyard into their library, then draws
//! seven cards. You untap up to six lands.
// End of the generated card header.

use crate::prelude::*;

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
pub static CARD: CardDef = Printed::TimeSpiral
    .abilities(&[Ability::spell().effect(&sequence(&[
        exile(this()),
        shuffle_into(
            each_player(),
            Objects::CardsIn {
                players: &each_player(),
                zones: &[Zone::Hand, Zone::Graveyard],
            },
        ),
        draw(each_player(), 7),
        untap_up_to(
            you(),
            6,
            Objects::All {
                zone: Zone::Battlefield,
                filter: Filter::Types(Types::LAND),
            },
        ),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// Six lands to cast it with, all tapped by the casting.
    const SIX_LANDS: &[&str] = &[
        Card::Island,
        Card::Island,
        Card::Plains,
        Card::Plains,
        Card::Plains,
        Card::Plains,
    ];

    /// P1's tapped Tundra is a land P0 may untap too. Interchangeable lands are one choice each
    /// time: after two Islands no Island is left to offer.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Time Spiral: exiled, the wheel, and chosen lands untapped")
            .battlefield(P0, SIX_LANDS)
            .hand(P0, &[Card::TimeSpiral, Card::Island])
            .graveyard(P0, &[Card::Plains])
            .library(P0, &[Card::Island; 6])
            .permanents(P1, &[Permanent::new(Card::Tundra).tapped()])
            .hand(P1, &[Card::Plains])
            .graveyard(P1, &[Card::Island])
            .library(P1, &[Card::Plains; 6])
            .actions(&[
                ME.casts(Card::TimeSpiral),
                ME.deciding(DecisionKind::ChooseUpTo, 4),
                ME.picks_up_to(Card::Island),
                ME.picks_up_to(Card::Island),
                ME.deciding(DecisionKind::ChooseUpTo, 3),
                ME.picks_up_to(Card::Plains),
                ME.done_picking(),
                ME.tapped(Card::Island, 0),
                ME.tapped(Card::Plains, 3),
                THEM.tapped(Card::Tundra, 1),
                ME.has(Zone::Exile, Card::TimeSpiral, 1),
                ME.has_cards(Zone::Graveyard, 0),
                ME.has_cards(Zone::Hand, 7),
                ME.has_cards(Zone::Library, 1),
                THEM.has_cards(Zone::Graveyard, 0),
                THEM.has_cards(Zone::Hand, 7),
                THEM.has_cards(Zone::Library, 1),
            ]),
        Scenario::new("Time Spiral with empty graveyards: six lands untapped, any player's")
            .battlefield(P0, SIX_LANDS)
            .hand(P0, &[Card::TimeSpiral])
            .library(P0, &[Card::Island; 7])
            .permanents(P1, &[Permanent::new(Card::Tundra).tapped()])
            .library(P1, &[Card::Plains; 7])
            .actions(&[
                ME.casts(Card::TimeSpiral),
                ME.picks_up_to(Card::Tundra),
                ME.picks_up_to(Card::Plains),
                ME.picks_up_to(Card::Plains),
                ME.picks_up_to(Card::Plains),
                ME.picks_up_to(Card::Plains),
                ME.picks_up_to(Card::Island),
                // Six chosen: no more is asked.
                ME.tapped(Card::Island, 1),
                ME.tapped(Card::Plains, 0),
                THEM.tapped(Card::Tundra, 0),
                ME.has_cards(Zone::Graveyard, 0),
                ME.has_cards(Zone::Hand, 7),
                THEM.has_cards(Zone::Hand, 7),
            ]),
    ];
}
