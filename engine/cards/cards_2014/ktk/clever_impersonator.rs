// Generated from the card store by `just card header`: notes go below the end marker.
//! Clever Impersonator {2}{U}{U}
//! Creature — Shapeshifter, 0/0
//!
//! You may have this creature enter as a copy of any nonland permanent on the battlefield.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::CleverImpersonator
    .abilities(&[Ability::replacing(&Replacement::enter_as_copy(
        &nonland(),
        None,
    ))])
    .card();
