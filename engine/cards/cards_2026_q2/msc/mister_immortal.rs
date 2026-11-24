// Generated from the card store by `just card header`: notes go below the end marker.
//! Mister Immortal {1}{G}
//! Legendary Creature — Mutant Hero, 2/1
//!
//! {2}{G}: Return this card from your graveyard or from exile to the battlefield tapped.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::MisterImmortal
    .abilities(&[Ability::activated(Cost::mana(mana("{2}{G}")))
        .in_zones(ZoneSet::GRAVEYARD.union(ZoneSet::EXILE))
        .effect(&put_onto_battlefield(&PutOnto {
            objects: this(),
            under: None,
            tapped: true,
            linked: false,
        }))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const IMMORTAL: &str = Card::MisterImmortal;
    const ACTIONS: &[Action] = &[
        ME.activates(IMMORTAL, 0),
        Resolve,
        ME.controls(IMMORTAL, 1),
        ME.tapped(IMMORTAL, 1),
    ];

    const fn return_tapped(name: &'static str) -> Scenario {
        Scenario::new(name)
            .battlefield(P0, &[Card::Forest; 3])
            .actions(ACTIONS)
    }

    pub const SCENARIOS: &[Scenario] = &[
        return_tapped("Mister Immortal returns tapped from the graveyard")
            .graveyard(P0, &[IMMORTAL]),
        return_tapped("Mister Immortal returns tapped from exile").exile(P0, &[IMMORTAL]),
    ];
}
