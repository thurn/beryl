// Generated from the card store by `just card header`: notes go below the end marker.
//! Time Warp {3}{U}{U}
//! Sorcery
//!
//! Target player takes an extra turn after this one.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::TimeWarp
    .abilities(&[Ability::spell()
        .target(&TargetSpec::player())
        .effect(&extra_turn(target_player()))])
    .card();
