// Generated from the card store by `just card header`: notes go below the end marker.
//! Skycloud Expanse
//! Land
//!
//! {1}, {T}: Add {W}{U}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::SkycloudExpanse
    .abilities(&[Ability::mana_of(
        Cost::of(ManaCost::generic(1), &[CostPart::TAP]),
        &[Term::one(ManaColor::White), Term::one(ManaColor::Blue)],
    )])
    .card();
