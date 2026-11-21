// Generated from the card store by `just card header`: notes go below the end marker.
//! Mana Tithe {W}
//! Instant
//!
//! Counter target spell unless its controller pays {1}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ManaTithe
    .abilities(&[Ability::spell().target(&TargetSpec::spell()).effect(
        &unless_its_controller_pays(&counter(target()), ManaCost::generic(1)),
    )])
    .card();
