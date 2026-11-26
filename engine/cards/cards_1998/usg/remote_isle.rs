// Generated from the card store by `just card header`: notes go below the end marker.
//! Remote Isle
//! Land
//!
//! This land enters tapped.
//! {T}: Add {U}.
//! Cycling {2} ({2}, Discard this card: Draw a card.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::RemoteIsle
    .abilities(&[
        Ability::replacing(&Replacement::ENTERS_TAPPED),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
        Ability::cycling(mana("{2}")),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[Scenario::new(
        "cycling Remote Isle puts it into the shared graveyard and draws from the shared library",
    )
    .shared_library(&[Card::Plains])
    .battlefield(P0, &[Card::Island, Card::Island])
    .hand(P0, &[Card::RemoteIsle])
    .actions(&[
        ME.activates(Card::RemoteIsle, 2),
        Resolve,
        ME.has(Zone::Graveyard, Card::RemoteIsle, 1),
        THEM.has(Zone::Graveyard, Card::RemoteIsle, 1),
        ME.has(Zone::Hand, Card::Plains, 1),
        ME.has_cards(Zone::Library, 0),
    ])];
}
