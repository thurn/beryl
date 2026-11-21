// Generated from the card store by `just card header`: notes go below the end marker.
//! Floodfarm Verge
//! Land
//!
//! {T}: Add {W}.
//! {T}: Add {U}. Activate only if you control a Plains or an Island.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::FloodfarmVerge
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::White)),
        Ability::tap_for_mana(ManaSet::one(ManaColor::Blue)).if_you_control(&Filter::AnyOf(&[
            Filter::Subtype(Subtype::Plains),
            Filter::Subtype(Subtype::Island),
        ])),
    ])
    .card();
