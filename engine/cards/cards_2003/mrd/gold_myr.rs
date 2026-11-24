// Generated from the card store by `just card header`: notes go below the end marker.
//! Gold Myr {2}
//! Artifact Creature — Myr, 1/1
//!
//! {T}: Add {W}.
// End of the generated card header.

use crate::prelude::*;

/// A mana creature for scenarios such as Humility removing its abilities.
pub static CARD: CardDef = Printed::GoldMyr
    .abilities(&[Ability::tap_for_mana(ManaSet::one(ManaColor::White))])
    .card();
