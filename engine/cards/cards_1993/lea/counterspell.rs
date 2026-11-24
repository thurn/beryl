// Generated from the card store by `just card header`: notes go below the end marker.
//! Counterspell {U}{U}
//! Instant
//!
//! Counter target spell.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Counterspell
    .abilities(&[Ability::spell()
        .target(&TargetSpec::spell())
        .effect(&counter(target()))])
    .card();
