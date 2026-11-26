// Generated from the card store by `just card header`: notes go below the end marker.
//! Predict {1}{U}
//! Instant
//!
//! Choose a card name, then target player mills a card. If a card with the chosen name was milled
//! this way, you draw two cards. Otherwise, you draw a card.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Predict
    .abilities(&[Ability::spell()
        .target(&TargetSpec::player())
        .effect(&sequence(&[
            choose_value(you(), ChosenKind::CardName),
            mill(target_player(), 1),
            if_else(
                Condition::HasChosenName(Objects::Those),
                &draw(you(), 2),
                &draw(you(), 1),
            ),
        ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const PREDICT: &str = Card::Predict;
    const BOLT: &str = Card::LightningBolt;

    /// P0 casts Predict on P1, whose library has Lightning Bolt on top. P0's own library
    /// holds the cards P0 draws.
    const fn predict(name: &'static str) -> Scenario {
        Scenario::new(name)
            .interaction(&[BOLT])
            .battlefield(P0, &[Card::Island, Card::Island])
            .hand(P0, &[PREDICT])
            .library(P0, &[Card::Island, Card::Island, Card::Island])
            .library(P1, &[BOLT, Card::Plains])
    }

    pub const SCENARIOS: &[Scenario] = &[
        predict("Predict naming the milled card draws two").actions(&[
            ME.casts(PREDICT),
            ME.targets_player(THEM),
            Resolve,
            ME.names(BOLT),
            THEM.has(Zone::Graveyard, BOLT, 1),
            ME.has_cards(Zone::Hand, 2),
            THEM.has_cards(Zone::Hand, 0),
        ]),
        predict("Predict naming another card draws one").actions(&[
            ME.casts(PREDICT),
            ME.targets_player(THEM),
            Resolve,
            ME.names_another_card(),
            THEM.has(Zone::Graveyard, BOLT, 1),
            ME.has_cards(Zone::Hand, 1),
        ]),
        // The name is that of a card in the game, but not of the milled one.
        predict("Predict naming a card that was not milled draws one").actions(&[
            ME.casts(PREDICT),
            ME.targets_player(THEM),
            Resolve,
            ME.names(Card::Plains),
            THEM.has(Zone::Graveyard, BOLT, 1),
            ME.has_cards(Zone::Hand, 1),
        ]),
        // P0 may target themself: the milled card is P0's own top card.
        Scenario::new("Predict can mill its caster's own library")
            .interaction(&[BOLT])
            .battlefield(P0, &[Card::Island, Card::Island])
            .hand(P0, &[PREDICT])
            .library(P0, &[BOLT, Card::Island, Card::Island])
            .actions(&[
                ME.casts(PREDICT),
                ME.targets_player(ME),
                Resolve,
                ME.names(BOLT),
                ME.has(Zone::Graveyard, BOLT, 1),
                ME.has_cards(Zone::Hand, 2),
            ]),
        // Rest in Peace exiles the milled card instead. The card is still found where it went,
        // a public zone (CR 701.17c), so naming it draws two.
        predict("Predict finds a milled card that Rest in Peace exiled")
            .interaction(&[BOLT, Card::RestInPeace])
            .battlefield(P0, &[Card::Island, Card::Island, Card::RestInPeace])
            .actions(&[
                ME.casts(PREDICT),
                ME.targets_player(THEM),
                Resolve,
                ME.names(BOLT),
                THEM.has(Zone::Exile, BOLT, 1),
                ME.has_cards(Zone::Hand, 2),
            ]),
    ];
}
