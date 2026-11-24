// Generated from the card store by `just card header`: notes go below the end marker.
//! No More Lies {W}{U}
//! Instant
//!
//! Counter target spell unless its controller pays {3}. If that spell is countered this way, exile
//! it instead of putting it into its owner's graveyard.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::NoMoreLies
    .abilities(&[Ability::spell().target(&TargetSpec::spell()).effect(
        &unless_its_controller_pays(&counter_and_exile(target()), ManaCost::generic(3)),
    )])
    .card();
