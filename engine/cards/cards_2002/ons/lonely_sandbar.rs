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
