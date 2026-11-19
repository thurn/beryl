// Generated from the card store by `just card header`: notes go below the end marker.
//! Counterspell {U}{U}
//! Instant
//!
//! Counter target spell.
// End of the generated card header.

// If the target spell has left the stack by the time Counterspell resolves,
// its only target is illegal and Counterspell does nothing.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Counterspell
    .abilities(&[Ability::spell()
        .target(&TargetSpec::spell())
        .effect(&counter(target()))])
    .card();
