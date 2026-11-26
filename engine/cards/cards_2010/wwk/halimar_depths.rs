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
