// Generated from the card store by `just card header`: notes go below the end marker.
//! Mental Note {U}
//! Instant
//!
//! Mill two cards.
//! Draw a card.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::MentalNote
    .abilities(&[Ability::spell().effect(&sequence(&[mill(you(), 2), draw(you(), 1)]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[Scenario::new(
        "Mental Note mills two cards into the shared graveyard and draws from the shared library",
    )
    .shared_library(&[Card::Plains, Card::Swamp, Card::Island])
    .battlefield(P0, &[Card::Island])
    .hand(P0, &[Card::MentalNote])
    .actions(&[
        ME.casts(Card::MentalNote),
        Resolve,
        ME.has(Zone::Graveyard, Card::Plains, 1),
        THEM.has(Zone::Graveyard, Card::Plains, 1),
        ME.has(Zone::Graveyard, Card::Swamp, 1),
        ME.has(Zone::Hand, Card::Island, 1),
        ME.has_cards(Zone::Library, 0),
    ])];
}
