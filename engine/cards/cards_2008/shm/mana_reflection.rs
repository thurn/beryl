// Generated from the card store by `just card header`: notes go below the end marker.
//! Mana Reflection {4}{G}{G}
//! Enchantment
//!
//! If you tap a permanent for mana, it produces twice as much of that mana instead.
// End of the generated card header.

use crate::prelude::*;

static DOUBLE_MANA: ManaChange = ManaChange::Multiplies {
    tapped: Filter::Any,
    by: 2,
};

pub static CARD: CardDef = Printed::ManaReflection
    .abilities(&[Ability::mana_change(&DOUBLE_MANA)])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Under Mana Reflection every land is a source the mana solver's plain classes cannot
        // describe. Lands of one card once made one such source each, so seventeen of them
        // were more than a payment holds in any layout and the game stalled.
        Scenario::new("Mana Reflection with seventeen lands of seven cards")
            .interaction(&[Card::SkycloudExpanse])
            .battlefield(
                P0,
                &[
                    Card::ManaReflection,
                    Card::SkycloudExpanse,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Forest,
                    Card::Forest,
                    Card::Forest,
                    Card::SnowCoveredIsland,
                    Card::SnowCoveredIsland,
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Swamp,
                    Card::Swamp,
                    Card::Swamp,
                ],
            )
            .hand(P0, &[Card::Island])
            .library(P0, &[Card::Island; 7])
            .actions(&[ME.plays(Card::Island), ME.controls(Card::Island, 3)]),
    ];
}
