// Generated from the card store by `just card header`: notes go below the end marker.
//! Lonely Sandbar
//! Land
//!
//! This land enters tapped.
//! {T}: Add {U}.
//! Cycling {U} ({U}, Discard this card: Draw a card.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::LonelySandbar
    .abilities(&[
        Ability::replacing(&Replacement::ENTERS_TAPPED),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        Ability::cycling(mana("{U}")),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new(
            "cycling Lonely Sandbar puts it into the shared graveyard and draws from the shared library",
        )
        .shared_library(&[Card::Plains])
        .battlefield(P0, &[Card::Island])
        .hand(P0, &[Card::LonelySandbar])
        .actions(&[
            ME.activates(Card::LonelySandbar, 2),
            Resolve,
            ME.has(Zone::Graveyard, Card::LonelySandbar, 1),
            THEM.has(Zone::Graveyard, Card::LonelySandbar, 1),
            ME.has(Zone::Hand, Card::Plains, 1),
            ME.has_cards(Zone::Library, 0),
        ]),
    ];
}
