// Generated from the card store by `just card header`: notes go below the end marker.
//! Dissolve {1}{U}{U}
//! Instant
//!
//! Counter target spell. Scry 1. (Look at the top card of your library. You may put that card on
//! the bottom.)
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::Dissolve
    .abilities(&[Ability::spell()
        .target(&TargetSpec::spell())
        .effect(&sequence(&[counter(target()), scry(you(), 1)]))])
    .card();
