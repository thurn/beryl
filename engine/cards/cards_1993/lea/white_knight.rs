// Generated from the card store by `just card header`: notes go below the end marker.
//! White Knight {W}{W}
//! Creature — Human Knight, 2/2
//!
//! First strike (This creature deals combat damage before creatures without first strike.)
//! Protection from black (This creature can't be blocked, targeted, dealt damage, or enchanted by
//! anything black.)
// End of the generated card header.

use crate::prelude::*;

/// A first striker with protection from black, implemented for Aura scenarios.
pub static CARD: CardDef = Printed::WhiteKnight
    .keywords(Keywords::FIRST_STRIKE)
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: this(),
        condition: None,
        modifications: &[Modification::AddProtection(Protection::Colors(
            Colors::BLACK,
        ))],
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // A black targeted spell can't choose White Knight even when another creature is legal.
        Scenario::new("White Knight can't be targeted by a black spell")
            .interaction(&[Card::Deathmark])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::WhiteKnight, Prop::Flier])
            .battlefield(P1, &[Card::Swamp])
            .hand(P1, &[Card::Deathmark])
            .actions(&[
                THEM.casts(Card::Deathmark),
                Resolve,
                ME.controls(Card::WhiteKnight, 1),
                ME.has(Zone::Graveyard, Prop::Flier, 1),
            ]),
    ];
}
