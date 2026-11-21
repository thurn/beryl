// Generated from the card store by `just card header`: notes go below the end marker.
//! Lightning Bolt {R}
//! Instant
//!
//! Lightning Bolt deals 3 damage to any target.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::LightningBolt
    .abilities(&[Ability::spell()
        .target(&TargetSpec::any_target())
        .effect(&deal_damage_to_any_target(3))])
    .card();
