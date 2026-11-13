// Generated from the card store by `just card header`: notes go below the end marker.
//! Timetwister {2}{U}
//! Sorcery
//!
//! Each player shuffles their hand and graveyard into their library, then draws seven cards. (Then
//! put Timetwister into its owner's graveyard.)
// End of the generated card header.

use crate::prelude::*;

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
pub static CARD: CardDef = Printed::Timetwister
    .abilities(&[Ability::spell().effect(&sequence(&[
        shuffle_into(
            each_player(),
            Objects::CardsIn {
                players: &each_player(),
                zones: &[Zone::Hand, Zone::Graveyard],
            },
        ),
        draw(each_player(), 7),
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// Each player ends with seven cards in hand and the rest of hand, graveyard and library in
    /// the library; Timetwister alone goes to the graveyard, after the shuffle.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("Timetwister with empty graveyards")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[Card::Timetwister, Card::Plains])
            .library(P0, &[Card::Plains; 7])
            .hand(P1, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island; 6])
            .actions(&[
                ME.casts(Card::Timetwister),
                ME.has_cards(Zone::Hand, 7),
                ME.has_cards(Zone::Library, 1),
                ME.has(Zone::Graveyard, Card::Timetwister, 1),
                ME.has_cards(Zone::Graveyard, 1),
                THEM.has_cards(Zone::Hand, 7),
                THEM.has_cards(Zone::Library, 1),
            ]),
        Scenario::new("Timetwister shuffles graveyards away")
            .battlefield(P0, &[Card::Island; 3])
            .hand(P0, &[Card::Timetwister])
            .graveyard(P0, &[Prop::Instant, Card::Oust])
            .library(P0, &[Card::Plains; 5])
            .graveyard(P1, &[Prop::Instant])
            .library(P1, &[Card::Island; 8])
            .actions(&[
                ME.casts(Card::Timetwister),
                ME.has_cards(Zone::Hand, 7),
                ME.has_cards(Zone::Library, 0),
                ME.has_cards(Zone::Graveyard, 1),
                THEM.has_cards(Zone::Hand, 7),
                THEM.has_cards(Zone::Graveyard, 0),
                THEM.has_cards(Zone::Library, 2),
            ]),
    ];
}
