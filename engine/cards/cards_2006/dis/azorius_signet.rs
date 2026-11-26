// Generated from the card store by `just card header`: notes go below the end marker.
//! Azorius Signet {2}
//! Artifact
//!
//! {1}, {T}: Add {W}{U}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::AzoriusSignet
    .abilities(&[Ability::mana_of(
        Cost::of(ManaCost::generic(1), &[CostPart::TAP]),
        &[Term::one(ManaColor::White), Term::one(ManaColor::Blue)],
    )])
    .card();
