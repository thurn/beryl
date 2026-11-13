// Generated from the card store by `just card header`: notes go below the end marker.
//! Swamp
//! Basic Land — Swamp
//!
//! ({T}: Add {B}.)
// End of the generated card header.

use crate::prelude::*;

/// [CR 305.6]: "An object with the land card type and a basic land type has the intrinsic ability
/// '{T}: Add [mana symbol],' ... for Swamps, {B}".
///
/// [CR 305.6]: https://yawgatog.com/resources/magic-rules/#R3056
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
        .battlefield(P1, &[Card::EliteSpellbinder])
        .actions(&[
            ME.casts(Card::Damn),
            Resolve,
            ME.tapped(Card::Swamp, 2),
            THEM.has(Zone::Graveyard, Card::EliteSpellbinder, 1),
        ])];
}
