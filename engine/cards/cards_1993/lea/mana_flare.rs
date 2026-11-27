// Generated from the card store by `just card header`: notes go below the end marker.
//! Mana Flare {2}{R}
//! Enchantment
//!
//! Whenever a player taps a land for mana, that player adds one mana of any type that land
//! produced.
// End of the generated card header.

use crate::prelude::*;

static EXTRA_MANA: ManaChange = ManaChange::Adds {
    tapped: Tapped::A(Filter::Types(Types::LAND)),
    added: Added::TypeProduced,
};

pub static CARD: CardDef = Printed::ManaFlare
    .abilities(&[Ability::mana_change(&EXTRA_MANA)])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Under Mana Flare one Island makes {U}{U}, enough for Chart a Course's {1}{U}. Cavern
        // of Souls in a library makes this a game with restricted mana, whose references
        // once counted one mana for each land for a spell and so missed Mana Flare's.
        Scenario::new("Mana Flare's mana pays for a spell in a game with restricted mana")
            .interaction(&[Card::ChartACourse, Card::CavernOfSouls])
            .battlefield(P0, &[Card::ManaFlare, Card::Island])
            .hand(P0, &[Card::ChartACourse])
            .library(P0, &[Card::Island; 7])
            .library(P1, &[Card::CavernOfSouls, Card::Island, Card::Island])
            .actions(&[
                ME.casts(Card::ChartACourse),
                ME.has(Zone::Graveyard, Card::ChartACourse, 1),
            ]),
        // Under the opponent's Mana Flare every land is a source the mana solver's classes
        // cannot say, and the reference checks list the plans of each cost at every position.
        // A search that tried every subset of the lands before reaching the Mountain took
        // a minute to list the plans of {R} on this board.
        Scenario::new("Plans of a cost under an opponent's Mana Flare with many lands")
            .interaction(&[
                Card::PanglacialWurm,
                Card::AzoriusSignet,
                Card::CavernOfSouls,
                Card::FloodfarmVerge,
                Card::SkycloudExpanse,
                Card::UrborgTombOfYawgmoth,
            ])
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Forest,
                    Card::UrborgTombOfYawgmoth,
                    Card::SnowCoveredIsland,
                    Card::SkycloudExpanse,
                    Card::Island,
                    Card::Mountain,
                    Card::FloodfarmVerge,
                    Card::Forest,
                    Card::Plains,
                    Card::Island,
                    Card::Swamp,
                    Card::AzoriusSignet,
                ],
            )
            .battlefield(P1, &[Card::ManaFlare])
            .hand(P0, &[Card::PanglacialWurm, Card::Island])
            .library(P0, &[Card::Island; 7])
            .library(P1, &[Card::CavernOfSouls, Card::Island, Card::Island])
            .actions(&[ME.plays(Card::Island), ME.tapped(Card::Island, 0)]),
    ];
}
