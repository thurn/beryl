// Generated from the card store by `just card header`: notes go below the end marker.
//! Tarfire {R}
//! Kindred Instant — Goblin
//!
//! Tarfire deals 2 damage to any target.
// End of the generated card header.

use crate::prelude::*;

/// Implemented for scenarios that count kindred and instant as two card types.
pub static CARD: CardDef = Printed::Tarfire
    .abilities(&[Ability::spell()
        .target(&TargetSpec::any_target())
        .effect(&deal_damage_to_any_target(2))])
    .card();
