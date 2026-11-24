// Generated from the card store by `just card header`: notes go below the end marker.
//! Chains of Mephistopheles {1}{B}
//! Enchantment
//!
//! If a player would draw a card except the first one they draw in each of their draw steps, that
//! player discards a card instead. If the player discards a card this way, they draw a card. If the
//! player doesn't discard a card this way, they mill a card.
// End of the generated card header.

use crate::prelude::*;

/// The draw after discarding is part of the original draw event. The same Chains can't
/// replace it again ([CR 614.5]): "it gets only one opportunity to affect an event or any
/// modified events that may replace that event." A second Chains or dredge can still
/// replace the modified draw.
///
/// [CR 614.5]: https://yawgatog.com/resources/magic-rules/#R6145
pub static CARD: CardDef = Printed::ChainsOfMephistopheles
    .abilities(&[
        Ability::replacing(&Replacement::draws_except_first_in_draw_step(
            Players::EachPlayer,
        ))
        .effect(&sequence(&[
            discard(that_player(), 1),
            if_else(
                Condition::Did,
                &draw(that_player(), 1),
                &mill(that_player(), 1),
            ),
        ])),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const CHAINS: &str = Card::ChainsOfMephistopheles;
    const DRESS: &str = Card::DressDown;
    const LOAM: &str = Card::LifeFromTheLoam;

    /// Mana for Dress Down, whose enters trigger draws a card.
    const ISLANDS: &[&str] = &[Card::Island, Card::Island];

    /// Mana for Timetwister.
    const THREE_ISLANDS: &[&str] = &[Card::Island, Card::Island, Card::Island];

    /// Five cards, so a dredge 3 leaves some to draw.
    const LIBRARY: &[&str] = &[
        Card::Island,
        Card::Plains,
        Card::Counterspell,
        Card::Island,
        Card::Plains,
    ];

    /// Eight cards, enough to draw seven from.
    const EIGHT_ISLANDS: &[&str] = &[
        Card::Island,
        Card::Island,
        Card::Island,
        Card::Island,
        Card::Island,
        Card::Island,
        Card::Island,
        Card::Island,
    ];

    const EIGHT_PLAINS: &[&str] = &[
        Card::Plains,
        Card::Plains,
        Card::Plains,
        Card::Plains,
        Card::Plains,
        Card::Plains,
        Card::Plains,
        Card::Plains,
    ];

    pub const SCENARIOS: &[Scenario] = &[
        // The opponent's Chains replaces P0's draw: the only card in hand is discarded, then
        // a card is drawn.
        Scenario::new("a draw with a card in hand discards it, then draws")
            .interaction(&[DRESS])
            .battlefield(P0, ISLANDS)
            .battlefield(P1, &[CHAINS])
            .hand(P0, &[DRESS, Card::Plains])
            .library(P0, LIBRARY)
            .actions(&[
                ME.casts(DRESS),
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has(Zone::Hand, Card::Island, 1),
                ME.has_cards(Zone::Hand, 1),
                ME.library_top(&[Card::Plains]),
            ]),
        // With nothing to discard, the player mills a card and draws nothing.
        Scenario::new("a draw with an empty hand mills a card instead")
            .interaction(&[DRESS])
            .battlefield(P0, ISLANDS)
            .battlefield(P1, &[CHAINS])
            .hand(P0, &[DRESS])
            .library(P0, LIBRARY)
            .actions(&[
                ME.casts(DRESS),
                Resolve,
                Resolve,
                ME.has_cards(Zone::Hand, 0),
                ME.has(Zone::Graveyard, Card::Island, 1),
                ME.library_top(&[Card::Plains]),
            ]),
        // The draw step's draw is the first of it, and is not replaced; a draw later in the
        // same step is.
        Scenario::new("the draw step's first draw is not replaced, the next one is")
            .interaction(&[DRESS])
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, ISLANDS)
            .battlefield(P1, &[CHAINS])
            .hand(P0, &[DRESS])
            .library(P0, LIBRARY)
            .actions(&[
                ME.passes(),
                ME.has(Zone::Hand, Card::Island, 1),
                ME.has_cards(Zone::Graveyard, 0),
                ME.casts(DRESS),
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, Card::Island, 1),
                ME.has(Zone::Hand, Card::Plains, 1),
                ME.has_cards(Zone::Hand, 1),
            ]),
        // The draw step's draw is dredged, so no card was drawn: the next draw of the step
        // is the first card drawn in it, and is not replaced either (CR 614.6).
        Scenario::new("after a dredged draw step draw, the next draw is the first")
            .interaction(&[DRESS, LOAM])
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, ISLANDS)
            .battlefield(P1, &[CHAINS])
            .hand(P0, &[DRESS])
            .graveyard(P0, &[LOAM])
            .library(P0, LIBRARY)
            .actions(&[
                ME.passes(),
                ME.picks_up_to(LOAM),
                ME.has(Zone::Hand, LOAM, 1),
                ME.casts(DRESS),
                Resolve,
                Resolve,
                ME.has(Zone::Hand, Card::Island, 1),
                ME.has(Zone::Hand, LOAM, 1),
                ME.has_cards(Zone::Graveyard, 3),
            ]),
        // Each of the fourteen draws is replaced, the opponent's on P0's turn too. Their
        // hands were shuffled away, so each of them mills seven.
        Scenario::new("Timetwister's draws with empty hands mill seven each")
            .battlefield(P0, THREE_ISLANDS)
            .battlefield(P1, &[CHAINS])
            .hand(P0, &[Card::Timetwister])
            .library(P0, EIGHT_ISLANDS)
            .library(P1, EIGHT_PLAINS)
            .actions(&[
                ME.casts(Card::Timetwister),
                Resolve,
                ME.has_cards(Zone::Hand, 0),
                ME.has_cards(Zone::Library, 1),
                ME.has(Zone::Graveyard, Card::Island, 7),
                THEM.has_cards(Zone::Hand, 0),
                THEM.has_cards(Zone::Library, 1),
                THEM.has(Zone::Graveyard, Card::Plains, 7),
            ]),
        // Seven draws, each replaced on its own: the card in hand is discarded and another
        // drawn, seven times.
        Scenario::new("drawing seven with a card in hand discards and draws seven times")
            .interaction(&[Card::JaceWielderOfMysteries])
            .permanents(
                P0,
                &[Permanent::new(Card::JaceWielderOfMysteries).counters(CounterKind::Loyalty, 9)],
            )
            .battlefield(P0, &[CHAINS])
            .hand(P0, &[Card::Plains])
            .library(P0, EIGHT_ISLANDS)
            .actions(&[
                ME.activates(Card::JaceWielderOfMysteries, 2), // −8
                Resolve,
                ME.has_cards(Zone::Hand, 1),
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has(Zone::Graveyard, Card::Island, 6),
                ME.has_cards(Zone::Library, 1),
            ]),
        // The first Chains has the card discarded; the second replaces the draw that follows,
        // and with the hand empty the player mills a card and draws nothing.
        Scenario::new("two Chains: discard, then mill")
            .interaction(&[DRESS])
            .battlefield(P0, ISLANDS)
            .battlefield(P1, &[CHAINS, CHAINS])
            .hand(P0, &[DRESS, Card::Plains])
            .library(P0, LIBRARY)
            .actions(&[
                ME.casts(DRESS),
                Resolve,
                Resolve,
                ME.has_cards(Zone::Hand, 0),
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has(Zone::Graveyard, Card::Island, 1),
                ME.library_top(&[Card::Plains]),
            ]),
        // Dredge applied first replaces the draw, and Chains has no draw left to replace
        // (CR 616.1f).
        Scenario::new("dredge before Chains")
            .interaction(&[DRESS, LOAM])
            .battlefield(P0, ISLANDS)
            .battlefield(P1, &[CHAINS])
            .hand(P0, &[DRESS, Card::Plains])
            .graveyard(P0, &[LOAM])
            .library(P0, LIBRARY)
            .actions(&[
                ME.casts(DRESS),
                Resolve,
                Resolve,
                ME.picks_up_to(LOAM),
                ME.has(Zone::Hand, LOAM, 1),
                ME.has(Zone::Hand, Card::Plains, 1),
                ME.has_cards(Zone::Graveyard, 3),
                ME.library_top(&[Card::Island]),
            ]),
        // Chains applied first has the card discarded, and dredge replaces the draw that
        // follows.
        // The planeswalker is something P0 could do afterwards, so the game stops there and
        // not at the end step, where Dress Down is sacrificed.
        Scenario::new("Chains before dredge")
            .interaction(&[DRESS, LOAM])
            .battlefield(P0, &[Card::Island, Card::Island, Prop::Planeswalker])
            .battlefield(P1, &[CHAINS])
            .hand(P0, &[DRESS, Card::Plains])
            .graveyard(P0, &[LOAM])
            .library(P0, LIBRARY)
            .actions(&[
                ME.casts(DRESS),
                Resolve,
                Resolve,
                ME.done_picking(),
                ME.picks_up_to(LOAM),
                ME.has(Zone::Hand, LOAM, 1),
                ME.has_cards(Zone::Hand, 1),
                // The discarded Plains, and one of the three cards milled.
                ME.has(Zone::Graveyard, Card::Plains, 2),
                ME.has_cards(Zone::Graveyard, 4),
                ME.library_top(&[Card::Island]),
            ]),
        // The draw is replaced by a mill, so a player with an empty library and an empty hand
        // never draws from it and doesn't lose (CR 614.6: the replaced draw never happens;
        // CR 704.5b: a player loses only if they attempted to draw from an empty library).
        Scenario::new("an empty library and an empty hand: the replaced draw doesn't lose")
            .interaction(&[DRESS])
            .battlefield(P0, ISLANDS)
            .battlefield(P1, &[CHAINS])
            .hand(P0, &[DRESS])
            .library(P0, &[])
            .actions(&[
                ME.casts(DRESS),
                Resolve,
                Resolve,
                ME.has_cards(Zone::Hand, 0),
                ME.has_cards(Zone::Library, 0),
                ME.has_cards(Zone::Graveyard, 0),
            ]),
        // A draw in the upkeep is not in the draw step, so it is replaced; the draw step's
        // draw is still the first one drawn in that step (CR 504.1).
        Scenario::new("an upkeep draw is replaced, the draw step's draw is not")
            .interaction(&[DRESS])
            .at(3, P0, TurnStep::Upkeep)
            .battlefield(P0, ISLANDS)
            .battlefield(P1, &[CHAINS])
            .hand(P0, &[DRESS, Card::Plains])
            .library(P0, LIBRARY)
            .actions(&[
                ME.casts(DRESS),
                Resolve,
                Resolve,
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has(Zone::Hand, Card::Island, 1),
                ME.passes(),
                ME.has(Zone::Hand, Card::Plains, 1),
                ME.has_cards(Zone::Hand, 2),
                ME.has_cards(Zone::Graveyard, 1),
            ]),
    ];
}
