// Generated from the card store by `just card header`: notes go below the end marker.
//! Reverse Engineer {3}{U}{U}
//! Sorcery
//!
//! Improvise (Your artifacts can help cast this spell. Each artifact you tap after you're done
//! activating mana abilities pays for {1}.)
//! Draw three cards.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ReverseEngineer
    .keywords(Keywords::IMPROVISE)
    .abilities(&[Ability::spell().effect(&draw(you(), 3))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const ENGINEER: &str = Card::ReverseEngineer;

    pub const SCENARIOS: &[Scenario] = &[
        // The cost is {3}{U}{U} and only two Islands make mana. Three artifacts pay the generic
        // {3} through improvise, so the spell can be cast and draws three cards.
        Scenario::new("improvise pays the generic part of the cost")
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Prop::ColorlessCreature,
                    Prop::ColorlessCreature,
                    Prop::ColorlessCreature,
                ],
            )
            .hand(P0, &[ENGINEER])
            .library(P0, &[Card::Island, Card::Island, Card::Island])
            .actions(&[
                ME.casts(ENGINEER),
                ME.tapped(Prop::ColorlessCreature, 3),
                ME.tapped(Card::Island, 2),
                Resolve,
                ME.has(Zone::Hand, Card::Island, 3),
            ]),
    ];
}
