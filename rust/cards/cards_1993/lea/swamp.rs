// Generated from the card store by `just card header`: notes go below the end marker.
//! Swamp
//! Basic Land — Swamp
//!
//! ({T}: Add {B}.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Swamp
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::Black))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// Two Swamps pay Damn's {B}{B}, which two Plains could not: Damn is the one card that asks
    /// for black mana.
    pub const SCENARIOS: &[Scenario] = &[Scenario::new("two Swamps pay {B}{B}")
        .interaction(&[Card::Damn])
        .battlefield(P0, &[Card::Swamp, Card::Swamp])
        .hand(P0, &[Card::Damn])
        .battlefield(P1, &[Prop::Flier])
        .actions(&[
            ME.casts(Card::Damn),
            Resolve,
            ME.tapped(Card::Swamp, 2),
            THEM.has(Zone::Graveyard, Prop::Flier, 1),
        ])];
}
