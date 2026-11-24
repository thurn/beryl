// Generated from the card store by `just card header`: notes go below the end marker.
//! Spara's Headquarters
//! Land — Forest Plains Island
//!
//! ({T}: Add {G}, {W}, or {U}.)
//! This land enters tapped.
//! Cycling {3} ({3}, Discard this card: Draw a card.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::SparasHeadquarters
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::Green)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        Ability::replacing(&Replacement::ENTERS_TAPPED),
        Ability::cycling(mana("{3}")),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("Spara's Headquarters cannot pay for a black creature")
                .battlefield(P0, &[Card::SparasHeadquarters, Card::Plains, Card::Plains])
                .hand(P0, &[Prop::FlashCreature, Card::Island])
                .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
        ];
}
