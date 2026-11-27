// Generated from the card store by `just card header`: notes go below the end marker.
//! Azorius Signet {2}
//! Artifact
//!
//! {1}, {T}: Add {W}{U}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::AzoriusSignet
    .abilities(&[Ability::mana_of(
        Cost::of(ManaCost::generic(1), &[CostPart::TAP]),
        &[Term::one(ManaColor::White), Term::one(ManaColor::Blue)],
    )])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Beza costs {2}{W}{W}. One Mountain pays the Signet's {1} for {W}{U}, the other
        // makes {R}, and Cavern of Souls makes the second {W}, which it may only for an
        // Elemental creature spell, not for the Signet's price. The payment search once left
        // out the Cavern's colored mana whenever it activated a priced mana ability.
        Scenario::new("Azorius Signet and Cavern of Souls together pay for an Elemental")
            .interaction(&[Card::BezaTheBoundingSpring, Card::CavernOfSouls])
            .battlefield(P0, &[Card::Mountain, Card::Mountain, Card::AzoriusSignet])
            .hand(P0, &[Card::CavernOfSouls, Card::BezaTheBoundingSpring])
            .library(P0, &[Card::Island; 7])
            .library(P1, &[Card::Island; 7])
            .actions(&[
                ME.plays(Card::CavernOfSouls),
                ME.chooses_named("Elemental"),
                ME.casts(Card::BezaTheBoundingSpring),
                ME.tapped(Card::CavernOfSouls, 1),
                ME.tapped(Card::AzoriusSignet, 1),
                Resolve,
                ME.controls(Card::BezaTheBoundingSpring, 1),
            ]),
        // Plains pays the Signet's {1}, so Tundra and the Signet's {W}{U} pay {2}{U}. Cavern
        // of Souls in a library makes this a game with restricted mana, whose references
        // once counted only one tap of each source for a spell and so missed the Signet.
        Scenario::new("Azorius Signet's mana pays for a spell in a game with restricted mana")
            .interaction(&[Card::StockUp, Card::CavernOfSouls])
            .battlefield(P0, &[Card::Tundra, Card::Plains, Card::AzoriusSignet])
            .hand(P0, &[Card::StockUp])
            .library(P0, &[Card::Island; 7])
            .library(P1, &[Card::CavernOfSouls, Card::Island, Card::Island])
            .actions(&[ME.casts(Card::StockUp), ME.tapped(Card::AzoriusSignet, 1)]),
    ];
}
