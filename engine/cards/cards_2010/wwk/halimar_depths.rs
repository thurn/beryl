// Generated from the card store by `just card header`: notes go below the end marker.
//! Halimar Depths
//! Land
//!
//! This land enters tapped.
//! When this land enters, look at the top three cards of your library, then put them back in any
//! order.
//! {T}: Add {U}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::HalimarDepths
    .abilities(&[
        Ability::replacing(&Replacement::ENTERS_TAPPED),
        when_enters(&reorder_top(you(), 3)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[Scenario::new(
        "Halimar Depths in a shared game reorders the top of the shared library for the opponent",
    )
    .shared_library(&[Card::Plains, Card::Swamp, Card::Island])
    .hand(P0, &[Card::HalimarDepths])
    .actions(&[
        ME.plays(Card::HalimarDepths),
        Resolve,
        ME.picks(Card::Island),
        ME.picks(Card::Swamp),
        advance_to(2, TurnStep::PrecombatMain),
        THEM.has(Zone::Hand, Card::Island, 1),
        ME.library_top(&[Card::Swamp, Card::Plains]),
    ])];
}
