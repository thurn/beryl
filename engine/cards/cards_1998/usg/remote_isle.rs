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
