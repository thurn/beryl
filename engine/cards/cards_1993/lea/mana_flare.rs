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
        // Under Mana Flare one Mountain makes {R}{R}, and two creatures pay the {2} of Stoke the
        // Flames by convoke. The casting steps' check for convoke once counted only plain
        // sources, which a land under Mana Flare is not, so the spell was not offered.
        Scenario::new("Mana Flare's mana and two creatures pay for a convoke spell")
            .interaction(&[Card::StokeTheFlames])
            .battlefield(
                P0,
                &[
                    Card::ManaFlare,
                    Card::Mountain,
                    Prop::ColorlessCreature,
                    Prop::ColorlessCreature,
                ],
            )
            .hand(P0, &[Card::StokeTheFlames])
            .library(P0, &[Card::Island; 7])
            .actions(&[
                ME.casts(Card::StokeTheFlames),
                ME.targets_player(THEM),
                ME.tapped(Prop::ColorlessCreature, 2),
                ME.tapped(Card::Mountain, 1),
                Resolve,
                THEM.life(16),
            ]),
        // An opponent holds a convoke spell under Mana Flare with six lands: the reference for
        // spells that creatures help pay for once counted one mana for each land, and the
        // casting steps' check counted Mana Flare's extra mana.
        Scenario::new("A convoke spell under Mana Flare")
            .interaction(&[Card::StokeTheFlames])
            .battlefield(P0, &[Card::Mountain, Card::Mountain, Card::Mountain])
            .battlefield(
                P1,
                &[
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                ],
            )
            .hand(P0, &[Card::ManaFlare])
            .hand(P1, &[Card::StokeTheFlames])
            .library(P0, &[Card::Island; 7])
            .library(P1, &[Card::Island; 7])
            .actions(&[
                ME.casts(Card::ManaFlare),
                Resolve,
                ME.has(Zone::Battlefield, Card::ManaFlare, 1),
            ]),
        // Under Mana Flare, Cavern of Souls tapped for its restricted ability produces one
        // restricted mana of the chosen color plus one unrestricted mana of that color from
        // Mana Flare: enough with an Island to cast Flickerwisp ({1}{W}{W}), and the spell
        // still gets Cavern's uncounterable rider.
        Scenario::new("Cavern of Souls under Mana Flare makes two mana for the chosen type")
            .interaction(&[Card::CavernOfSouls, Card::Flickerwisp])
            .battlefield(P0, &[Card::ManaFlare, Card::Island])
            .hand(P0, &[Card::CavernOfSouls, Card::Flickerwisp])
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Card::Counterspell])
            .library(P0, &[Card::Island; 7])
            .library(P1, &[Card::Island; 7])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Elemental"),
                ME.casts(Card::Flickerwisp),
                THEM.casts(Card::Counterspell),
                Resolve,
                Resolve,
                ME.controls(Card::Flickerwisp, 1),
            ]),
    ];
}
